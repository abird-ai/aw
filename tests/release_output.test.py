"""Release-helper filesystem behavior; Nix builds are separately executed gates."""
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
ASSETS = ('aw-linux-x86_64', 'aw-linux-aarch64', 'aw-windows-x86_64.exe',
          'aw-windows-aarch64.exe', 'aw-macos-aarch64')


class ReleaseOutputTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='aw-release-output-')
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.repo = self.root / 'repo'
        (self.repo / 'scripts').mkdir(parents=True)
        for name in ('build-release-artifacts.sh', 'release-output.py'):
            source = ROOT / 'scripts' / name
            if source.exists():
                shutil.copyfile(source, self.repo / 'scripts' / name)
        for name in ('install.sh', 'install.ps1'):
            (self.repo / name).write_text('fixture installer\n')
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        for name in ASSETS:
            (self.assets / name).write_text('new binary\n')
            (self.assets / (name + '.sha256')).write_text('fixture checksum\n')
        for name in ('VERSION', 'PLATFORMS.txt', 'release-manifest.json'):
            (self.assets / name).write_text('new fixture metadata\n')
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        nix = self.bin / 'nix'
        nix.write_text('#!/bin/sh\nprintf called > "$NIX_MARKER"\n'
                       'if [ "${FAIL_BUILD:-0}" = 1 ]; then exit 7; fi\n'
                       'if [ -n "${ADD_UNRELATED:-}" ]; then printf retain > "$ADD_UNRELATED"; fi\n'
                       'printf "%s\\n" "$FIXTURE_ASSETS"\n')
        nix.chmod(0o755)
        self.output = self.root / "output 'quoted'"
        self.env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                    'FIXTURE_ASSETS': str(self.assets), 'NIX_MARKER': str(self.root / 'nix.called')}

    def run_helper(self, **extra):
        return subprocess.run(['bash', str(self.repo / 'scripts/build-release-artifacts.sh'),
                               str(self.output)], env={**self.env, **extra},
                              capture_output=True, text=True, timeout=10)

    def snapshot(self, directory):
        return {p.name: p.read_bytes() for p in directory.iterdir()}

    def test_unrelated_output_is_preserved_and_refused_before_build(self):
        self.output.mkdir()
        sentinel = self.output / 'irreplaceable.txt'
        sentinel.write_text('private fixture data\n')
        result = self.run_helper()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(sentinel.read_text(), 'private fixture data\n')
        self.assertFalse((self.root / 'nix.called').exists())

    def test_fresh_output_and_generated_output_replacement(self):
        result = self.run_helper()
        self.assertEqual(result.returncode, 0, result.stderr)
        before = self.snapshot(self.output)
        self.assertEqual(len(before), 15)
        (self.assets / ASSETS[0]).write_text('replacement binary\n')
        result = self.run_helper()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.output / ASSETS[0]).read_text(), 'replacement binary\n')
        self.assertEqual(set(before), set(self.snapshot(self.output)))
        self.assertEqual(list(self.root.glob('.output*.tmp.*')), [])

    def test_build_failure_preserves_previous_output(self):
        self.assertEqual(self.run_helper().returncode, 0)
        before = self.snapshot(self.output)
        self.assertNotEqual(self.run_helper(FAIL_BUILD='1').returncode, 0)
        self.assertEqual(self.snapshot(self.output), before)

    def test_output_changed_during_build_is_preserved(self):
        self.assertEqual(self.run_helper().returncode, 0)
        before = self.snapshot(self.output)
        result = self.run_helper(ADD_UNRELATED=str(self.output / 'concurrent.txt'))
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.snapshot(self.output), {**before, 'concurrent.txt': b'retain'})

    def test_readers_never_observe_a_missing_output_during_replacement(self):
        self.assertEqual(self.run_helper().returncode, 0)
        stop = threading.Event()
        errors = []

        def read_output():
            while not stop.is_set():
                try:
                    self.assertEqual((self.output / 'VERSION').read_text(), 'new fixture metadata\n')
                except Exception as error:
                    errors.append(error)
                    return

        reader = threading.Thread(target=read_output)
        reader.start()
        try:
            for _ in range(6):
                result = self.run_helper()
                self.assertEqual(result.returncode, 0, result.stderr)
        finally:
            stop.set()
            reader.join(timeout=5)
        self.assertFalse(reader.is_alive())
        self.assertEqual(errors, [])

    def test_incomplete_artifacts_preserve_previous_output(self):
        self.assertEqual(self.run_helper().returncode, 0)
        before = self.snapshot(self.output)
        (self.assets / ASSETS[0]).unlink()
        result = self.run_helper()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.snapshot(self.output), before)

    def test_output_symlink_is_preserved(self):
        target = self.root / 'target'; target.mkdir()
        (target / 'private').write_text('retain')
        self.output.symlink_to(target, target_is_directory=True)
        self.assertNotEqual(self.run_helper().returncode, 0)
        self.assertTrue(self.output.is_symlink())
        self.assertEqual((target / 'private').read_text(), 'retain')

    def test_generated_output_with_symlink_is_preserved(self):
        self.assertEqual(self.run_helper().returncode, 0)
        target = self.root / 'private'; target.write_text('retain')
        (self.output / 'VERSION').unlink()
        (self.output / 'VERSION').symlink_to(target)
        self.assertNotEqual(self.run_helper().returncode, 0)
        self.assertTrue((self.output / 'VERSION').is_symlink())
        self.assertEqual(target.read_text(), 'retain')


if __name__ == '__main__':
    unittest.main()
