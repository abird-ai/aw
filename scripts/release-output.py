"""Linux release output admission and atomic publication; no recursive deletion."""
import ctypes
import os
import pathlib
import stat
import sys

ASSETS = {
    'aw-linux-x86_64', 'aw-linux-aarch64', 'aw-windows-x86_64.exe',
    'aw-windows-aarch64.exe', 'aw-macos-aarch64',
}
FILES = ASSETS | {name + '.sha256' for name in ASSETS} | {
    'VERSION', 'PLATFORMS.txt', 'release-manifest.json', 'install.sh', 'install.ps1',
}


def entries(path):
    if path.is_symlink() or not path.is_dir():
        raise ValueError(f'Release output must be a directory, not a file or symlink: {path}')
    found = {entry.name for entry in path.iterdir()}
    if not found <= FILES:
        raise ValueError(f'Refusing unrelated files in release output: {path}')
    for name in found:
        info = (path / name).lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError(f'Refusing linked or non-regular release entry: {path / name}')
    return found


def check(path):
    if not os.path.lexists(path):
        return
    found = entries(path)
    if found and found != FILES:
        raise ValueError(f'Refusing incomplete or unrelated release output: {path}')


def cleanup(path):
    # Inspect the entire flat set before unlinking anything. Unknown entries
    # retain the directory for inspection, including after a successful swap.
    for name in entries(path):
        (path / name).unlink()
    path.rmdir()


def publish(stage, output):
    if entries(stage) != FILES:
        raise ValueError(f'Incomplete staged release; previous output preserved: {stage}')
    check(output)
    # Both paths have the same parent/filesystem. RENAME_EXCHANGE replaces an
    # existing directory atomically; NOREPLACE protects a newly created path.
    # Unsupported libc/kernel/filesystem fails before modifying either path.
    libc = ctypes.CDLL(None, use_errno=True)
    rename = getattr(libc, 'renameat2', None)
    if rename is None:
        raise ValueError('Atomic release publication requires Linux renameat2')
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int
    flags = 2 if os.path.lexists(output) else 1
    if rename(-100, os.fsencode(stage), -100, os.fsencode(output), flags) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), str(output))
    if flags == 2:
        try:
            cleanup(stage)
        except (OSError, ValueError) as error:
            print(f'Published {output}; retained previous output at {stage}: {error}', file=sys.stderr)


if __name__ == '__main__':
    try:
        action, *paths = sys.argv[1:]
        if action == 'check' and len(paths) == 1:
            check(pathlib.Path(paths[0]))
        elif action == 'cleanup' and len(paths) == 1:
            cleanup(pathlib.Path(paths[0]))
        elif action == 'publish' and len(paths) == 2:
            publish(*(pathlib.Path(path) for path in paths))
        else:
            raise ValueError('usage: release-output.py check|cleanup PATH | publish STAGE OUTPUT')
    except (OSError, ValueError) as error:
        raise SystemExit(str(error)) from error
