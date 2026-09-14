"""Cloud desktop and optional Minecraft rendering checks; never signs in or hosts a world."""
import argparse,json,os,pathlib,re,subprocess,tempfile,time,urllib.request,shutil
p=argparse.ArgumentParser();p.add_argument('binary',type=pathlib.Path);p.add_argument('--game',action='store_true');p.add_argument('--report',type=pathlib.Path,required=True);a=p.parse_args()
report={'desktop':'not run','minecraft':'not run','platform':os.sys.platform}
with tempfile.TemporaryDirectory(prefix='blocklink-cloud-',ignore_cleanup_errors=True) as temp:
 root=pathlib.Path(temp)/'data';root.mkdir();health=pathlib.Path(temp)/'ready'
 env=dict(os.environ,BLOCKLINK_UPDATE_HEALTH=str(health),WEBVIEW2_USER_DATA_FOLDER=str(pathlib.Path(temp)/'webview'))
 proc=subprocess.Popen([str(a.binary.resolve()),'--data-dir',str(root)],env=env)
 ident=None;game_pid=None
 def rpc(action,payload={}):
  ep=json.loads((root/'service.json').read_text());req=urllib.request.Request('http://127.0.0.1:'+str(ep['port'])+'/rpc',data=json.dumps({'action':action,'payload':payload}).encode(),headers={'Authorization':'Bearer '+ep['token']})
  value=json.load(urllib.request.urlopen(req,timeout=30))
  if not value['ok']:raise RuntimeError(value.get('error'))
  return value['value']
 def job(action,payload):
  jobid=rpc(action,payload)['jobId'];deadline=time.time()+600
  while time.time()<deadline:
   found=next(j for j in rpc('status')['jobs'] if j['id']==jobid)
   if found['status']=='done':return found.get('result',{})
   if found['status'] in ('error','cancelled'):raise RuntimeError(found.get('message'))
   time.sleep(1)
  raise TimeoutError(action)
 try:
  for _ in range(120):
   if proc.poll() is not None:raise RuntimeError('Desktop exited before rendering')
   if health.exists():break
   time.sleep(1)
  else:raise TimeoutError('Frontend did not confirm readiness')
  assert rpc('status')['serviceProtocol']==1;report['desktop']='passed: rendered frontend and authenticated service'
  print(report['desktop'],flush=True)
  if a.game:
   ident=job('create',{'name':'Cloud rendering check','minecraft':'1.21.1','loader':'fabric','memory':2048,'install':True})['id']
   report['minecraft']='installed: Minecraft 1.21.1 and Fabric'
   job('offline-profile',{'name':'CloudCheck'});result=job('launch',{'id':ident});assert result.get('pid'),result;game_pid=result['pid']
   log=root/'instances'/ident/'game/logs/latest.log'
   for _ in range(120):
    text=log.read_text(errors='replace') if log.exists() else ''
    if re.search(r'Created:.*textures/atlas',text) and 'Backend library: LWJGL' in text:break
    state=next(i for i in rpc('status')['instances'] if i['instance']['instanceId']==ident)
    if not state['running']:raise RuntimeError('Minecraft exited before rendering: '+text[-1500:])
    time.sleep(1)
   else:raise TimeoutError('Minecraft rendering marker missing')
   report['minecraft']='passed: managed Java, Fabric installation and texture atlas rendering';print(report['minecraft'],flush=True)
 except Exception as e:
  report['error']=str(e)
  if game_pid:
   jcmd=shutil.which('jcmd')
   if jcmd:
    try:
     dump=subprocess.run([jcmd,str(game_pid),'Thread.print'],capture_output=True,text=True,timeout=15).stdout
     at=dump.find('"Render thread"');report['renderThread']=dump[at:at+2500] if at>=0 else dump[:1500]
    except Exception as diagnostic:report['threadDiagnostic']=str(diagnostic)
   if os.sys.platform=='darwin':
    try:
     gpu=subprocess.run(['system_profiler','SPDisplaysDataType'],capture_output=True,text=True,timeout=15).stdout
     report['graphics']=[line.strip() for line in gpu.splitlines() if any(key in line for key in ['Chipset Model','Metal Support','VRAM','Type:'])]
    except Exception:pass
   elif os.name=='nt':
    try:report['graphics']=subprocess.run(['powershell','-NoProfile','-Command','Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion | ConvertTo-Json -Compress'],capture_output=True,text=True,timeout=15).stdout
    except Exception:pass
  raise
 finally:
  if ident:
   for name,relative in [('gameLogTail','game/logs/latest.log'),('launchLogTail','latest.log')]:
    logpath=root/'instances'/ident/relative
    if logpath.exists():report[name]=logpath.read_text(errors='replace')[-6000:]
   try:rpc('stop',{'id':ident,'force':True})
   except Exception:pass
  try:rpc('prepare-app-update')
  except Exception:pass
  if proc.poll() is None:proc.terminate()
  try:proc.wait(timeout=10)
  except subprocess.TimeoutExpired:proc.kill();proc.wait()
  a.report.parent.mkdir(parents=True,exist_ok=True);a.report.write_text(json.dumps(report,indent=2)+'\n')
  summary=os.environ.get('GITHUB_STEP_SUMMARY')
  if summary:
   with open(summary,'a') as out:out.write('\n### Cloud UI check\n\n```json\n'+json.dumps(report,indent=2)+'\n```\n')
