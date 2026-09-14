"""Windows native update smoke test. Opens isolated launcher windows and closes them afterward.
Run with --new PATH --previous PATH --work-dir PATH on an interactive Windows desktop.
"""
import argparse,os,pathlib,subprocess,json,time,hashlib,shutil,uuid,urllib.request
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--new',required=True,type=pathlib.Path)
parser.add_argument('--previous',required=True,type=pathlib.Path)
parser.add_argument('--work-dir',required=True,type=pathlib.Path)
args=parser.parse_args()
if os.name!='nt':parser.error('This test requires Windows')
base=args.work_dir.resolve();base.mkdir(parents=True,exist_ok=True)
new=args.new.resolve();old=args.previous.resolve()
assert new.is_file() and old.is_file()
def processes():
 result=subprocess.run(['powershell','-NoProfile','-Command','Get-CimInstance Win32_Process -Filter "Name = \'Blocklink.exe\' OR Name = \'update-helper.exe\'" | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress'],capture_output=True,text=True)
 data=json.loads(result.stdout or '[]');return data if isinstance(data,list) else [data]
def stop(pid):
 subprocess.run(['powershell','-NoProfile','-Command',f'Stop-Process -Id {int(pid)} -Force -ErrorAction SilentlyContinue'],capture_output=True)
def rpc(root,action):
 endpoint=json.loads((root/'service.json').read_text())
 req=urllib.request.Request(f'http://127.0.0.1:{endpoint["port"]}/rpc',data=json.dumps({'action':action,'payload':{}}).encode(),headers={'Authorization':'Bearer '+endpoint['token']})
 return json.load(urllib.request.urlopen(req,timeout=5))
for crash in [False,True]:
 directory=base/('p0-update-'+uuid.uuid4().hex);directory.mkdir();root=directory/'data';root.mkdir()
 target=directory/'Blocklink.exe';helper=directory/'update-helper.exe';shutil.copy2(old,target);shutil.copy2(new,helper)
 old_service=None
 if not crash:
  old_service=subprocess.Popen([str(old),'--service',str(root)],creationflags=0x08000000)
  for _ in range(100):
   if (root/'service.json').exists():break
   time.sleep(.1)
 process=subprocess.Popen([str(helper),'--apply-launcher-update',str(target),str(root)],creationflags=0x08000000)
 try:
  if crash:
   for _ in range(60):
    children=[p for p in processes() if p['ParentProcessId']==process.pid and p['ExecutablePath'] and pathlib.Path(p['ExecutablePath'])==target]
    if children:stop(children[0]['ProcessId']);break
    time.sleep(.1)
   else:raise AssertionError('New launcher never started')
  assert process.wait(timeout=90)==0
  expected=old if crash else new
  assert hashlib.sha256(target.read_bytes()).digest()==hashlib.sha256(expected.read_bytes()).digest()
  if crash:assert 'restored' in (root/'update-error.txt').read_text()
  else:
   assert not list(directory.glob('.blocklink-previous-*'))
   assert rpc(root,'status')['value']['serviceProtocol']==1
   assert old_service.wait(timeout=10)==0
  print('PASS: native crash rollback' if crash else 'PASS: native update + rendered UI health confirmation',flush=True)
 finally:
  if old_service is not None and old_service.poll() is None:old_service.terminate();old_service.wait()
  if process.poll() is None:process.terminate();process.wait()
  try:rpc(root,'prepare-app-update')
  except Exception:pass
  for p in processes():
   if p['ExecutablePath'] and pathlib.Path(p['ExecutablePath']).parent==directory:stop(p['ProcessId'])
