"""Real Workspace shell -> inert broker -> source exit -> independent tty.

Private fixtures only. No provider request, tool grant, or existing user State.
"""
import errno
import fcntl
import json
import os
import pathlib
import pty
import select
import shutil
import signal
import socket
import struct
import sys
import subprocess
import tempfile
import termios
import time

assert os.uname().sysname == 'Linux'
mode = sys.argv[1] if len(sys.argv) > 1 else 'parser'
assert mode in ('parser', 'vim', 'less', 'source-crash')
parser_mode = mode in ('parser', 'source-crash')
root = pathlib.Path(tempfile.mkdtemp(prefix='awm-'))
work = root / 'work'; work.mkdir()
tmp = root / 'tmp'; tmp.mkdir(mode=0o700)
binary = os.environ.get('AW_TEST_BINARY', str(pathlib.Path(__file__).resolve().parents[1] / 'target/debug/aw'))
bash = shutil.which('bash')
env = {**os.environ, 'HOME': str(root), 'XDG_CONFIG_HOME': str(root),
       'XDG_DATA_HOME': str(root), 'XDG_STATE_HOME': str(root),
       'XDG_CACHE_HOME': str(root), 'TMPDIR': str(tmp), 'AW_BASH': bash,
       'AW_TEST_FAKE_KEY': 'synthetic_uncontacted',
       'AW_FAKE_PROVIDER_SECRET': 'must-not-enter-broker', 'TERM': 'xterm-256color', 'COLORTERM': ''}
for key in ('TMUX', 'TMUX_PANE', 'KITTY_WINDOW_ID', 'WEZTERM_PANE', 'GHOSTTY_RESOURCES_DIR', 'FOOT_CLIENT_PID'):
    env.pop(key, None)
config = root / 'config.toml'
config.write_text('default_profile="fixture"\n[profiles.fixture]\nkind="openai"\nbase_url="http://127.0.0.1:9/v1"\nmodel="fixture"\napi_key_env="AW_TEST_FAKE_KEY"\nallow_insecure_http=true\n')
(work / 'app.py').write_text(r'''
import os, pathlib, select, sys, termios, time, tty
original = termios.tcgetattr(0)
try:
    tty.setraw(0)
    os.write(1, b'MAIN_SCREEN\r\n' + b'old scroll\r\n' * 45)
    os.write(1, '\x1b[?1049h\x1b[?2004h\x1b[1;31mLIVE_STATE_界\x1b[0m'.encode())
    os.write(1, b'\x1b[3')
    pathlib.Path('app.ready').touch()
    end = time.monotonic() + 50
    while not pathlib.Path('continue').exists():
        if time.monotonic() > end: raise RuntimeError('fixture continuation timed out')
        time.sleep(.02)
    os.write(1, b'2mCONTINUED\x1b[0m\x1b[6n')
    data = b''
    while b'R' not in data:
        if not select.select([0], [], [], 5)[0]: raise RuntimeError('no live DSR response')
        data += os.read(0, 100)
    time.sleep(.12)
    while select.select([0], [], [], 0)[0]: data += os.read(0, 100)
    pathlib.Path('reply').write_bytes(data)
    os.write(1, b'\r\nDSR_ONCE\r\n')
    while not pathlib.Path('finish').exists(): time.sleep(.02)
finally:
    os.write(1, b'\x1b[?1049l')
    termios.tcsetattr(0, termios.TCSANOW, original)
''')
(work / 'fixture.txt').write_text('APPLICATION_MARKER_界\n' + ''.join(f'line {i} Unicode 界\n' for i in range(120)))
processes = []
masters = []
endpoint = None
broker_pid = None
passed = False

def start(args):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 120, 0, 0))
    before = termios.tcgetattr(slave)
    proc = subprocess.Popen([binary, *args], cwd=work, env=env, stdin=slave,
                            stdout=slave, stderr=slave, start_new_session=True)
    os.close(slave); processes.append(proc); masters.append(master)
    return proc, master, before, bytearray()

def drain(view, seconds=.12):
    proc, master, _, output = view
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if select.select([master], [], [], .03)[0]:
            try:
                block = os.read(master, 65536)
                if not block: break
                output.extend(block)
            except OSError as error:
                if error.errno == errno.EIO: break
                raise
        if proc.poll() is not None: break

def until(predicate, view=None, seconds=15):
    end = time.monotonic() + seconds
    while not predicate():
        if time.monotonic() >= end:
            raise RuntimeError('fixture timeout: ' + repr(bytes(view[3][-1800:])) if view else 'fixture timeout')
        if view: drain(view)
        else: time.sleep(.025)

def type_command(view, command):
    # Model human typing; bursts of >32 independent KeyEvents may correctly
    # overflow the bounded reactor queue. Never resend uncertain input.
    for byte in command.encode() + b'\r':
        os.write(view[1], bytes([byte])); time.sleep(.007)

def palette(view, command):
    os.write(view[1], b'\x1bOP'); drain(view, .22)
    os.write(view[1], command.encode() + b'\r'); drain(view, .25)

def detach(view):
    os.write(view[1], b'\x1b[24~')
    until(lambda: view[0].poll() is not None, view, 8)
    assert view[0].returncode == 0, bytes(view[3][-600:])
    assert termios.tcgetattr(view[1]) == view[2], 'recipient termios not restored'
    assert b'\x1b[?1049l' in view[3]

def invitation(): return json.loads((endpoint / 'invite').read_text())

def connect():
    code = invitation(); stream = socket.socket(socket.AF_UNIX)
    stream.settimeout(5); stream.connect(code['socket'])
    stream.sendall(json.dumps({'op': 'hello', 'version': 1, 'ticket': code['ticket']}).encode() + b'\n')
    reader = stream.makefile('rb')
    ready = json.loads(reader.readline()); assert ready['op'] == 'ready', ready
    return stream, reader, code

try:
    print('FIXTURE', root, flush=True)
    source = start(['--ui-mode', 'workspace', '--provider-profile', 'fixture',
                    '--config-file', str(config), '--state-dir', str(root/'state'),
                    '--workspace-directory', str(work), '--tools-mode', 'off'])
    until(lambda: b'Sessions' in source[3], source)
    palette(source, 'terminal-start')
    until(lambda: b'shell ready.' in source[3], source)
    application = 'python3 app.py' if parser_mode else ('vim -Nu NONE -i NONE -n fixture.txt' if mode == 'vim' else 'less -R fixture.txt')
    type_command(source, 'echo $$ > shell.pid; ' + application)
    if parser_mode: until(lambda: (work / 'app.ready').exists(), source)
    else: until(lambda: b'APPLICATION_MARKER' in source[3], source)
    shell_pid = int((work / 'shell.pid').read_text())
    time.sleep(.1); drain(source)
    palette(source, 'terminal-promote')
    def activated():
        global endpoint
        endpoints = list(tmp.glob('aw-pty-*/active'))
        if endpoints:
            endpoint = endpoints[0].parent
            return (endpoint / 'invite').exists()
        return False
    until(activated, source)
    transaction = json.loads((endpoint / 'transaction').read_text())
    broker_pid = transaction['broker_pid']
    assert transaction['source_pid'] == source[0].pid
    assert transaction['identity']['child_pid'] == shell_pid
    assert transaction['root'] == str(work)
    assert transaction['terminal'] == invitation()['terminal']
    assert (endpoint / 'released').read_bytes() == (endpoint / 'active').read_bytes()
    broker_env = (pathlib.Path('/proc') / str(broker_pid) / 'environ').read_bytes()
    assert b'AW_FAKE_PROVIDER_SECRET=' not in broker_env
    assert b'AW_TEST_FAKE_KEY=' not in broker_env
    os.write(source[1], b'touch SOURCE_MUST_NOT_EXECUTE\r'); drain(source, .2)
    palette(source, 'terminal-forget')
    until(lambda: b'Forgot' in source[3], source)
    if mode == 'source-crash':
        os.kill(source[0].pid, signal.SIGKILL)
        source[0].wait(timeout=4)
    else:
        palette(source, 'quit-discard')
        until(lambda: source[0].poll() is not None, source)
        assert source[0].returncode == 0
        assert termios.tcgetattr(source[1]) == source[2], 'Workspace termios not restored'
    assert not (work / 'SOURCE_MUST_NOT_EXECUTE').exists()
    assert (pathlib.Path('/proc') / str(shell_pid)).exists(), 'original shell died with Workspace'
    stream, reader, code = connect()
    try:
        first = json.loads(reader.readline())['snapshot']
        assert first['id'] == transaction['terminal']
        assert first['alternate_screen']
        text = ''.join(c['text'] for c in first['cells'])
        if parser_mode:
            assert first['bracketed_paste'] and 'LIVE_STATE_界' in text
            assert any(c['bold'] and c['fg'] == {'Indexed': 1} for c in first['cells']), 'lost attributes'
            (work / 'continue').touch()
            until(lambda: (work / 'reply').exists())
            reply = (work / 'reply').read_bytes()
            assert reply.startswith(b'\x1b[') and reply.count(b'R') == 1, reply
        else:
            assert 'APPLICATION_MARKER_界' in text, text
        stream.sendall(b'{"op":"detach"}\n')
        while json.loads(reader.readline())['op'] != 'detached': pass
    finally: reader.close(); stream.close()
    until(lambda: invitation()['ticket'] != code['ticket'])
    attached = start(['workspace', 'attach', str(endpoint / 'invite')])
    until(lambda: b'F12' in attached[3] and b'detach' in attached[3], attached)
    if parser_mode: (work / 'finish').touch()
    else: os.write(attached[1], b':q!\r' if mode == 'vim' else b'q')
    drain(attached, .5)
    type_command(attached, 'echo $$ > after.pid; echo MARK >> once')
    until(lambda: (work / 'after.pid').exists(), attached)
    assert int((work / 'after.pid').read_text()) == shell_pid
    type_command(attached, 'sleep 20; echo BAD > interrupted'); drain(attached, .25)
    os.write(attached[1], b'\x03'); drain(attached, .3)
    type_command(attached, 'echo CONTROL_RETAINED > control')
    until(lambda: (work / 'control').exists(), attached)
    assert not (work / 'interrupted').exists()
    detach(attached)
    until(lambda: invitation()['ticket'] != code['ticket'])
    assert (work / 'once').read_text() == 'MARK\n', 'input replayed'
    inspected = subprocess.run([binary, 'workspace', 'persistent', 'inspect', str(endpoint)], env=env,
                                capture_output=True, text=True, check=True)
    assert 'ACTIVE_RECORDED' in inspected.stdout
    stopped = subprocess.run([binary, 'workspace', 'persistent', 'stop', str(endpoint / 'invite')], env=env,
                              capture_output=True, text=True, timeout=8, check=True)
    until(lambda: not endpoint.exists())
    print('LIVE_MIGRATION_PASS', {'application': mode, 'originalShellPid': shell_pid, 'sourceExit': source[0].returncode,
          'sameRootOwnerId': True, 'alternateScreenUnicode': True, 'partialCSIAndDSR': parser_mode, 'exclusiveInput': True, 'noReplay': True,
          'ctrlC': True, 'recipientTermiosRestored': True, 'sourceTermiosRestored': mode != 'source-crash', 'explicitStop': True}, flush=True)
    passed = True
finally:
    if endpoint and (endpoint / 'invite').exists():
        subprocess.run([binary, 'workspace', 'persistent', 'stop', str(endpoint / 'invite')], env=env,
                       capture_output=True, timeout=8)
    for process in processes:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGKILL); process.wait(timeout=4)
    if broker_pid:
        try: os.kill(broker_pid, signal.SIGKILL)
        except ProcessLookupError: pass
    for master in masters: os.close(master)
    if passed: shutil.rmtree(root)
    else: print('FAILED_FIXTURE', root, flush=True)
