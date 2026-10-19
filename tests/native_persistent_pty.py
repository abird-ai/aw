"""Real opt-in detached PTY broker and independently attached client windows.

All files/processes are private synthetic fixtures, with no Agent, provider,
SQLite, tool effects, network interface or durable user grants.
"""
import errno
import fcntl
import json
import os
import shutil
import pathlib
import pty
import select
import signal
import struct
import subprocess
import tempfile
import termios
import time
import sys

mode=sys.argv[1] if len(sys.argv)>1 else 'reconnect'
assert mode in ('reconnect','broker-loss','competition','shell-exit','workspace-exit')
assert sys.platform.startswith('linux'), 'native persistent PTY process test requires Linux /proc'
root=pathlib.Path(tempfile.mkdtemp(prefix='aw-broker-host-fixture-'))
work=root/'work';work.mkdir()
socketbase=root/'tmp';socketbase.mkdir(mode=0o700)
(work/'fixture.txt').write_text('private explicit persistent shell fixture\n')
binary=os.environ.get('AW_TEST_BINARY',str(pathlib.Path(__file__).resolve().parents[1]/'target/debug/aw'))
bash=subprocess.run(['which','bash'],capture_output=True,text=True,check=True).stdout.strip()
env={**os.environ,'HOME':str(root),'XDG_CONFIG_HOME':str(root),
     'XDG_STATE_HOME':str(root),'XDG_DATA_HOME':str(root),
     'XDG_CACHE_HOME':str(root),'TMPDIR':str(socketbase),
     'AW_FAKE_PROVIDER_SECRET':'must-not-be-inherited',
     'TERM':'xterm-256color','COLORTERM':''}
# These independent PTYs emulate xterm, not the invoking desktop/tmux peer.
for key in ('TMUX','TMUX_PANE','KITTY_WINDOW_ID','WEZTERM_PANE',
            'GHOSTTY_RESOURCES_DIR','FOOT_CLIENT_PID'):
  env.pop(key,None)
try:
  launcher=subprocess.run([binary,'workspace','persistent','start',str(work),bash],
     cwd=str(work),env=env,capture_output=True,text=True,timeout=19,check=True)
except Exception:
  print('STARTUP_FIXTURE',root)
  raise
text=launcher.stdout
import re
match=re.search(r'(?m)^aw workspace attach (/[^\r\n]+/invite)$',text)
assert match, 'missing broker invitation, got '+repr(text[:300])
invite=pathlib.Path(match.group(1))
def read_code():
  return json.loads(invite.read_text())['ticket']
initial_token=read_code()
socket_path=invite.parent/'socket'
assert invite.stat().st_mode&0o077==0
assert invite.parent.stat().st_mode&0o077==0
assert socket_path.is_socket()
def owner_pids():
  result=[]
  for process in pathlib.Path('/proc').iterdir():
    if process.name.isdigit():
      try:
        args=(process/'cmdline').read_bytes().split(b'\0')
        strings=[x.decode(errors='replace') for x in args]
        if len(strings)>=8 and strings[0]==binary and strings[1:4]==['workspace','persistent','serve'] and str(invite.parent) in strings:
          result.append(int(process.name))
      except (PermissionError, FileNotFoundError, ProcessLookupError):
        pass
  return result
owners=owner_pids()
if len(owners)!=1:raise RuntimeError('expected exactly one dedicated owner, found '+repr(owners))
broker_pid=owners[0]
# The terminal-only broker must not inherit arbitrary credentials or hook
# settings supplied by the launcher.
broker_env=(pathlib.Path('/proc')/str(broker_pid)/'environ').read_bytes()
if b'AW_FAKE_PROVIDER_SECRET=' in broker_env:
  raise RuntimeError('broker inherited a launcher credential variable')

def live(pid):
  try:os.kill(pid,0);return True
  except ProcessLookupError:return False
def pty_start():
  master,slave=pty.openpty()
  fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',30,105,0,0))
  before=termios.tcgetattr(slave)
  child=subprocess.Popen([binary,'workspace','attach',str(invite)],
    cwd=str(work),env=env,stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
  os.close(slave)
  return child,master,before
def drain(master,child,seconds,payload):
  until=time.monotonic()+seconds
  while time.monotonic()<until:
    r,_,_=select.select([master],[],[],.045)
    if r:
      try:
        block=os.read(master,65536)
        if not block:break
        payload.extend(block)
      except OSError as error:
        if error.errno in (errno.EIO,errno.EBADF):break
        raise
    if child.poll() is not None:break
def wait_for_file(master,child,payload,path,seconds=8):
  end=time.monotonic()+seconds
  while time.monotonic()<end:
    drain(master,child,.11,payload)
    if path.is_file():return path.read_text().strip()
  raise RuntimeError('child failed to write '+str(path)+': '+repr(bytes(payload[-340:])))
def wait_rotate(previous,seconds=8):
  end=time.monotonic()+seconds
  while time.monotonic()<end:
    if invite.exists() and read_code()!=previous:return read_code()
    time.sleep(.055)
  raise RuntimeError('broker did not rotate consumed invitation')
def close_attachment(child,master,before,output,by='detach'):
  if by=='detach':
    os.write(master,b'\x1b[24~')
    end=time.monotonic()+7
    while time.monotonic()<end and child.poll() is None:
      drain(master,child,.12,output)
    if child.poll() is None:raise RuntimeError('F12 detach hung')
    if child.returncode!=0:raise RuntimeError('F12 returned '+str(child.returncode)+' '+repr(bytes(output[-320:])))
    after=termios.tcgetattr(master)
    if before!=after:raise RuntimeError('recipient termios not restored')
    if b'\x1b[?1049l' not in output:raise RuntimeError('receiver alternate screen not released')
  elif by=='crash':
    os.killpg(child.pid,signal.SIGKILL)
    child.wait(timeout=4)
  os.close(master)
def attached_shell(tag,crash=False):
  child,master,before=pty_start()
  output=bytearray()
  try:
    end=time.monotonic()+8
    while time.monotonic()<end:
      drain(master,child,.12,output)
      if b'F12 detach' in output or (b'F12' in output and b'detach' in output):
        break
    else:raise RuntimeError('attached view did not render original PTY: '+repr(bytes(output[-260:])))
    name=work/(tag+'.pid')
    os.write(master,('echo $$>'+name.name+'\r').encode())
    pid=wait_for_file(master,child,output,name)
    if not pid.isdigit():raise RuntimeError('shell returned nonnumeric PID '+repr(pid))
    os.write(master,b'\x03')
    drain(master,child,.25,output)
    if child.poll() is not None:raise RuntimeError('Ctrl+C unexpectedly closed recipient')
    close_attachment(child,master,before,output,'crash' if crash else 'detach')
    return pid,bytes(output)
  finally:
    if child.poll() is None:
      os.killpg(child.pid,signal.SIGTERM)
      try:child.wait(timeout=3)
      except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
    try:os.close(master)
    except OSError:pass
def stale_hello(code):
  import socket as sockets
  with sockets.socket(sockets.AF_UNIX,sockets.SOCK_STREAM) as connection:
    connection.settimeout(3)
    connection.connect(str(socket_path))
    connection.sendall(json.dumps({'op':'hello','version':1,'ticket':code}).encode()+b'\n')
    data=connection.recv(1024)
  return b'error' in data or b'Error' in data
passed=False
try:
  print('BROKER_STARTED',{'launcherExit':launcher.returncode,'brokerPid':broker_pid,'separateOwner':True,'fixture':str(root),
      'privateInvite':True,'idleHours':24},flush=True)
  if mode=='workspace-exit':
    # Closing an unrelated original Workspace must not affect this separately
    # owned shell. Synthetic configuration, no model or tool admission.
    cfg=root/'config.toml'
    cfg.write_text('default_profile="fixture"\n[profiles.fixture]\nkind="openai"\nbase_url="http://127.0.0.1:9/v1"\nmodel="fixture"\napi_key_env="AW_TEST_FAKE_KEY"\nallow_insecure_http=true\n')
    ui_env={**env,'AW_TEST_FAKE_KEY':'synthetic_uncontacted'}
    original_master,original_slave=pty.openpty()
    fcntl.ioctl(original_slave,termios.TIOCSWINSZ,struct.pack('HHHH',25,105,0,0))
    original_termios=termios.tcgetattr(original_slave)
    original_ui=subprocess.Popen([
      binary,'--ui-mode','workspace','--provider-profile','fixture',
      '--config-file',str(cfg),'--state-dir',str(root/'state'),
      '--workspace-directory',str(work),'--tools-mode','off',
    ],cwd=str(work),env=ui_env,stdin=original_slave,stdout=original_slave,
      stderr=original_slave,start_new_session=True)
    os.close(original_slave)
    ui_output=bytearray()
    try:
      deadline=time.monotonic()+7
      while time.monotonic()<deadline:
        drain(original_master,original_ui,.12,ui_output)
        if b'Sessions' in ui_output:break
      else:raise RuntimeError('independent Workspace did not render')
      os.write(original_master,b'\x1bOP')
      drain(original_master,original_ui,.35,ui_output)
      os.write(original_master,b'quit-discard\r')
      deadline=time.monotonic()+7
      while original_ui.poll() is None and time.monotonic()<deadline:
        drain(original_master,original_ui,.12,ui_output)
      if original_ui.returncode!=0 or termios.tcgetattr(original_master)!=original_termios:
        raise RuntimeError('original Workspace did not cleanly exit/restore termios')
      if not live(broker_pid):raise RuntimeError('separate broker died with original Workspace')
      print('ORIGINAL_WORKSPACE_EXIT',{'exit':0,'termiosRestored':True,'brokerAlive':True})
    finally:
      if original_ui.poll() is None:os.killpg(original_ui.pid,signal.SIGKILL);original_ui.wait()
      os.close(original_master)
  first,output1=attached_shell('first')
  assert live(broker_pid),'broker died when first frontend detached'
  second_token=wait_rotate(initial_token)
  assert second_token!=initial_token
  assert stale_hello(initial_token),'spent first ticket was unexpectedly valid'
  if mode=='competition':
    primary,master,original=pty_start()
    out=bytearray()
    try:
      end=time.monotonic()+7
      while time.monotonic()<end:
        drain(master,primary,.12,out)
        if b'F12 detach' in out or (b'F12' in out and b'detach' in out):break
      else:raise RuntimeError('competing primary did not attach')
      backup,backup_master,backup_before=pty_start()
      competing_output=bytearray()
      try:
        deadline=time.monotonic()+7
        while backup.poll() is None and time.monotonic()<deadline:
          drain(backup_master,backup,.12,competing_output)
        if backup.poll() is None or backup.returncode==0:
          raise RuntimeError('concurrent independently tty-equipped recipient was admitted or hung')
        if b'F12 detach' in competing_output:
          raise RuntimeError('competing recipient rendered active original PTY')
      finally:
        if backup.poll() is None:
          os.killpg(backup.pid,signal.SIGKILL);backup.wait()
        os.close(backup_master)
      close_attachment(primary,master,original,out)
      assert live(broker_pid)
    finally:
      if primary.poll() is None:
        os.killpg(primary.pid,signal.SIGKILL);primary.wait()
      try:os.close(master)
      except OSError:pass
    after_race=wait_rotate(second_token)
    assert stale_hello(second_token),'race reused original controller token'
    print('COMPETING_CLIENTS_DENIED',True,flush=True)

  if mode=='shell-exit':
    client,master,before=pty_start()
    payload=bytearray()
    try:
      deadline=time.monotonic()+8
      while time.monotonic()<deadline:
        drain(master,client,.13,payload)
        if b'F12 detach' in payload or (b'F12' in payload and b'detach' in payload):break
      else:raise RuntimeError('shell-exit receiver not admitted')
      os.write(master,b'exit\r')
      deadline=time.monotonic()+8
      while client.poll() is None and time.monotonic()<deadline:
        drain(master,client,.12,payload)
      if client.poll() is None or client.returncode==0:
        raise RuntimeError('shell exit was not propagated as owner shutdown')
      if termios.tcgetattr(master)!=before:raise RuntimeError('shell exit failed to restore recipient terminal')
      deadline=time.monotonic()+5
      while invite.exists() and time.monotonic()<deadline:time.sleep(.055)
      if invite.exists() or socket_path.exists():
        raise RuntimeError('exited shell left an active broker invitation')
      print('SHELL_EXIT_CLEANUP',{'noProcessReplay':True,'recipientNonzero':True,'endpointGone':True})
      passed=True
    finally:
      if client.poll() is None:os.killpg(client.pid,signal.SIGKILL);client.wait()
      os.close(master)
  elif mode=='broker-loss':
    token=read_code()
    receiver,master,before=pty_start()
    payload=bytearray()
    try:
      end=time.monotonic()+8
      while time.monotonic()<end:
        drain(master,receiver,.13,payload)
        if b'F12 detach' in payload or (b'F12' in payload and b'detach' in payload):break
      else:raise RuntimeError('owner-loss receiver was not admitted')
      os.kill(broker_pid,signal.SIGKILL)
      end=time.monotonic()+8
      while receiver.poll() is None and time.monotonic()<end:
        drain(master,receiver,.12,payload)
      if receiver.poll() is None or receiver.returncode==0:
        raise RuntimeError('owner loss did not fail the receiver closed')
      if termios.tcgetattr(master)!=before:raise RuntimeError('receiver did not restore termios on owner loss')
      print('OWNER_LOSS',{'receiverNonzero':True,'noShellReplay':True,'exit':receiver.returncode})
      passed=True
    finally:
      if receiver.poll() is None:os.killpg(receiver.pid,signal.SIGKILL);receiver.wait()
      os.close(master)
  elif mode in ('reconnect','competition','workspace-exit'):
    pre_second=read_code()
    second,output2=attached_shell('second',crash=True)
    if second!=first:raise RuntimeError('receiver crash restarted shell')
    third_token=wait_rotate(pre_second)
    third,output3=attached_shell('third')
    if third!=first:raise RuntimeError('persistent reattach restarted shell')
    assert live(broker_pid)
    fourth_token=wait_rotate(third_token)
    stopped=subprocess.run([binary,'workspace','persistent','stop',str(invite)],
      cwd=str(work),env=env,capture_output=True,text=True,timeout=14)
    if stopped.returncode!=0:
      raise RuntimeError('explicit stop not acknowledged: '+repr(stopped.stderr[-250:]))
    deadline=time.monotonic()+8
    while invite.exists() and time.monotonic()<deadline:time.sleep(.05)
    if invite.exists() or socket_path.exists():raise RuntimeError('broker failed to remove private endpoint')
    print('PERSISTENT_SHELL',{
      'passed':True,'sameShellPid':first,'threeFrontends':3,'receiverCrashRecovered':True,
      'tokensRotated':True,'oldTicketDenied':True,'stopAcknowledged':True,
      'endpointRemoved':True,'brokerOwnerPid':broker_pid,
      'streamBytes':[len(output1),len(output2),len(output3)]},flush=True)
    passed=True
finally:
  if mode!='broker-loss' and invite.exists():
    subprocess.run([binary,'workspace','persistent','stop',str(invite)],
      cwd=str(work),env=env,capture_output=True,timeout=9)
  if live(broker_pid):
    # Only the exact broker process created by this fixture; never broad kill.
    if broker_pid in owner_pids():
      os.kill(broker_pid,signal.SIGKILL)
  stopped = broker_pid not in owner_pids()
  if passed and stopped:
    # Clean only the exact random private paths created by this fixture.
    if invite.parent.parent == socketbase and invite.parent.name.startswith('aw-pty-'):
      if invite.parent.exists():shutil.rmtree(invite.parent)
    shutil.rmtree(root)
  print('PRIVATE_FIXTURES_CLEANED',passed and stopped,
        'BROKER_STOPPED',stopped,flush=True)
if not passed:raise SystemExit(1)
