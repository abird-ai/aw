"""Adversarial native broker admission with private synthetic Linux PTYs.

Happy-path Workspace/ChildGuard proof is native_live_migration.py. This harness
controls the sender to test malformed frames, identities, signed receipts and
both process-loss boundaries without adding production fault switches.
"""
import array
import hashlib
import json
import os
import pathlib
import pty
import secrets
import shutil
import signal
import socket
import struct
import sys
import subprocess
import tempfile
import time

binary = os.environ.get('AW_TEST_BINARY', str(pathlib.Path(__file__).resolve().parents[1] / 'target/debug/aw'))
root = pathlib.Path(tempfile.mkdtemp(prefix='amp-'))
root.chmod(0o700)
env = {**os.environ, 'TMPDIR': str(root)}
passed = False

def encode(value): return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()
def write_private(path, data):
    with open(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as file:
        file.write(data)
def exact(stream, size):
    value = b''
    while len(value) < size:
        block = stream.recv(size - len(value))
        if not block: raise EOFError('incomplete migration frame')
        value += block
    return value

def until(test, seconds=6):
    end = time.monotonic() + seconds
    while not test():
        if time.monotonic() >= end: raise RuntimeError('protocol fixture timed out')
        time.sleep(.02)

class Fixture:
    def __init__(self, mode):
        self.mode = mode
        self.path = root / ('aw-pty-' + secrets.token_hex(8)); self.path.mkdir(mode=0o700)
        self.master, slave = pty.openpty()
        self.shell = subprocess.Popen([shutil.which('bash'), '--noprofile', '--norc', '-c',
            "printf READY; IFS= read -r word; printf 'ACCEPTED:%s\\n' \"$word\"; IFS= read -r finish"],
            stdin=slave, stdout=slave, stderr=slave, cwd=self.path, start_new_session=True)
        os.close(slave)
        self.pidfd = os.pidfd_open(self.shell.pid)
        self.listener = socket.socket(socket.AF_UNIX); self.listener.bind(str(self.path / 'control'))
        self.listener.listen(); self.listener.settimeout(5)
        key = self.path / 'test-key'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519', '-out', str(key)], check=True, capture_output=True)
        key.chmod(0o600)
        pub = subprocess.run(['openssl', 'pkey', '-in', str(key), '-pubout', '-outform', 'DER'], check=True, capture_output=True).stdout[-32:]
        # Kernel metadata from the retained master, never a slave pathname open.
        info = pathlib.Path('/proc/self/fdinfo/' + str(self.master)).read_text()
        number = int(next(line.split(':')[1] for line in info.splitlines() if line.startswith('tty-index:')))
        self.nonce = secrets.token_bytes(32)
        self.checkpoint = {'format': 1, 'id': 'pty_synthetic_' + str(self.shell.pid), 'root': str(self.path),
                          'owner': 'synthetic-actor', 'site': 'local', 'epoch': 7, 'rows': 24, 'cols': 80,
                          'events': [], 'bytes': 0, 'pending_filter': {'state': 0, 'osc': False, 'escape': False, 'csi': [], 'utf8': 0}}
        self.wire = encode(self.checkpoint)
        self.broker = subprocess.Popen([binary, 'workspace', 'persistent', 'adopt', str(self.path)],
            env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True)
        self.tx = {'version': 1, 'nonce': list(self.nonce), 'source_pid': os.getpid(), 'broker_pid': self.broker.pid,
                   'terminal': self.checkpoint['id'], 'root': str(self.path), 'owner': 'synthetic-actor', 'site': 'local',
                   'epoch': 7, 'identity': {'child_pid': self.shell.pid, 'pty_number': number},
                   'checkpoint': list(hashlib.sha256(self.wire).digest()), 'release_key': list(pub)}
        if mode == 'wrong-source': self.tx['source_pid'] += 1
        if mode == 'wrong-recipient': self.tx['broker_pid'] += 1
        if mode == 'wrong-protocol': self.tx['version'] += 1
        for name in ('root', 'owner', 'site', 'terminal'):
            if mode == 'wrong-' + name: self.tx[name] += '-wrong'
        if mode == 'stale-epoch': self.tx['epoch'] += 1
        if mode == 'wrong-hash': self.tx['checkpoint'][0] ^= 1
        if mode == 'wrong-pidfd': self.tx['identity']['child_pid'] += 1
        if mode == 'wrong-master': self.tx['identity']['pty_number'] += 1
        write_private(self.path / 'transaction', encode(self.tx))
        self.stream = None
        self.ready = None

    def prepare(self):
        if self.mode in ('wrong-recipient', 'wrong-protocol'):
            return
        self.stream, _ = self.listener.accept(); self.stream.settimeout(5)
        if self.mode == 'wrong-source': return
        if self.mode == 'large-frame':
            self.stream.sendall(struct.pack('>Q', 2**64 - 1)); return
        if self.mode == 'truncated-frame':
            self.stream.sendall(struct.pack('>Q', 12) + b'{'); self.stream.close(); return
        self.stream.sendall(struct.pack('>Q', len(self.wire)) + self.wire)
        if self.mode not in ('wrong-source', 'wrong-root', 'wrong-owner', 'wrong-site', 'wrong-terminal', 'stale-epoch', 'wrong-hash'):
            proof = bytes([1] * 32) if self.mode == 'wrong-proof' else self.nonce
            fds = array.array('i', [self.master, self.pidfd])
            self.stream.sendmsg([b'\x02' + proof], [(socket.SOL_SOCKET, socket.SCM_RIGHTS, fds)])
        if self.mode in ('no-release', 'bad-signature', 'stale-release', 'lost-ready', 'receiver-crash-pre', 'lost-ack', 'duplicate-release', 'receiver-crash-post'):
            if self.mode != 'lost-ready':
                self.ready = json.loads(exact(self.stream, struct.unpack('>Q', exact(self.stream, 8))[0]))
            self.stream.close()

    def release(self):
        record = self.ready.copy()
        if self.mode == 'stale-release': record['epoch'] += 1
        message = b'aw-pty-source-released-v1\0' + encode(record)
        write_private(self.path / 'test-message', message)
        signature = subprocess.run(['openssl', 'pkeyutl', '-sign', '-rawin', '-inkey', str(self.path / 'test-key'),
                                    '-in', str(self.path / 'test-message')], capture_output=True, check=True).stdout
        if self.mode == 'bad-signature': signature = bytes([0] * 64)
        data = encode({'record': record, 'signature': list(signature)})
        write_private(self.path / 'released-next', data)
        os.replace(self.path / 'released-next', self.path / 'released')
        return data

    def original_usable(self):
        assert not (self.path / 'active').exists(), 'unreleased recipient activated'
        assert self.shell.poll() is None, 'inert receiver signalled original shell'
        os.write(self.master, b'accepted\n')
        import select
        output = b''; end = time.monotonic() + 4
        while b'ACCEPTED:accepted' not in output:
            assert time.monotonic() < end, output
            if select.select([self.master], [], [], .05)[0]: output += os.read(self.master, 4096)
        assert b'READY' in output, 'inert receiver consumed original PTY output'

    def close(self):
        if self.stream:
            try: self.stream.close()
            except OSError: pass
        self.listener.close()
        if self.broker.poll() is None: self.broker.kill()
        self.broker.wait(timeout=4)
        if self.shell.poll() is None:
            signal.pidfd_send_signal(self.pidfd, signal.SIGKILL)
        self.shell.wait(timeout=4)
        if self.master is not None: os.close(self.master)
        os.close(self.pidfd)
        self.broker.stderr.close()

if len(sys.argv) > 1 and sys.argv[1] == '--source-fixture':
    phase, progress, go = sys.argv[2:5]
    fixture = Fixture('lost-ready' if phase == 'escrow' else 'no-release')
    if phase != 'intent': fixture.prepare()
    write_private(pathlib.Path(progress), encode({'directory': str(fixture.path), 'broker': fixture.broker.pid, 'shell': fixture.shell.pid}))
    until(lambda: pathlib.Path(go).exists())
    if phase == 'released':
        os.close(fixture.master); fixture.master = None
        fixture.release()
    # Abrupt original-owner exit: no Python cleanup or original-master owner.
    os._exit(0)

def source_loss(phase):
    progress = root / ('source-' + phase)
    go = root / ('go-' + phase)
    child = subprocess.Popen([sys.executable, __file__, '--source-fixture', phase, str(progress), str(go)],
                             env={**env, 'AW_TEST_BINARY': binary}, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    handles = []
    try:
        until(progress.exists)
        data = json.loads(progress.read_text())
        endpoint = pathlib.Path(data['directory'])
        for pid in (data['broker'], data['shell']): handles.append(os.pidfd_open(pid))
        go.touch(); assert child.wait(timeout=6) == 0
        if phase == 'released':
            until(lambda: (endpoint / 'active').exists() and (endpoint / 'invite').exists())
            tx = json.loads((endpoint / 'transaction').read_text())
            assert tx['identity']['child_pid'] == data['shell']
            subprocess.run([binary, 'workspace', 'persistent', 'stop', str(endpoint / 'invite')],
                           env={**env, 'TMPDIR': str(endpoint.parent)}, check=True, capture_output=True, timeout=8)
        else:
            time.sleep(.35)
            assert not (endpoint / 'active').exists(), 'source death substituted for release'
        print('PROTOCOL_PASS source-crash-' + phase, flush=True)
    finally:
        if child.poll() is None: child.kill(); child.wait(timeout=4)
        child.stderr.close()
        for handle in handles:
            try: signal.pidfd_send_signal(handle, signal.SIGKILL)
            except ProcessLookupError: pass
            os.close(handle)

modes = ['wrong-protocol', 'wrong-root', 'wrong-owner', 'wrong-site', 'wrong-terminal', 'stale-epoch', 'wrong-hash',
         'wrong-pidfd', 'wrong-master', 'wrong-proof', 'wrong-source', 'wrong-recipient', 'large-frame', 'truncated-frame',
         'no-release', 'bad-signature', 'stale-release', 'lost-ready', 'receiver-crash-pre', 'lost-ack', 'duplicate-release', 'receiver-crash-post']
try:
    print('PROTOCOL_FIXTURE', root, flush=True)
    for mode in modes:
        fixture = Fixture(mode)
        try:
            fixture.prepare()
            if mode in ('bad-signature', 'stale-release'): fixture.release()
            if mode in ('lost-ack', 'duplicate-release', 'receiver-crash-post'):
                # Synthetic source closes its final master before signing;
                # production guard/reader closure is independently tested.
                os.close(fixture.master); fixture.master = None
                data = fixture.release()
                until(lambda: (fixture.path / 'active').exists() and (fixture.path / 'invite').exists())
                if mode == 'duplicate-release':
                    for _ in range(3):
                        (fixture.path / 'released').write_bytes(data); time.sleep(.025)
                    names = [(task / 'comm').read_text().strip() for task in (pathlib.Path('/proc') / str(fixture.broker.pid) / 'task').iterdir()]
                    assert names.count('aw-terminal-rea') == 1, names
                if mode == 'receiver-crash-post':
                    fixture.broker.kill(); fixture.broker.wait(timeout=4)
                    check = subprocess.run([binary, 'workspace', 'persistent', 'inspect', str(fixture.path)], env=env,
                                           capture_output=True, text=True, check=True)
                    assert 'ACTIVE_RECORDED' in check.stdout and 'verify liveness' in check.stdout
                else:
                    stopped = subprocess.run([binary, 'workspace', 'persistent', 'stop', str(fixture.path / 'invite')], env=env,
                                             capture_output=True, timeout=8, check=True)
            else:
                if mode == 'receiver-crash-pre': fixture.broker.kill(); fixture.broker.wait(timeout=4)
                elif mode.startswith('wrong-') or mode in ('stale-epoch', 'large-frame', 'truncated-frame'):
                    fixture.broker.wait(timeout=7)
                    assert fixture.broker.returncode != 0
                else: time.sleep(.25)
                fixture.original_usable()
            print('PROTOCOL_PASS', mode, flush=True)
        finally: fixture.close()
    for phase in ('intent', 'escrow', 'prepared', 'released'): source_loss(phase)
    passed = True
finally:
    if passed: shutil.rmtree(root)
    else: print('FAILED_PROTOCOL_FIXTURE', root, flush=True)
