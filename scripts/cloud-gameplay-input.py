"""Send actual X11 keyboard/mouse input to the isolated Minecraft client."""
import json,os,pathlib,subprocess,time
log=pathlib.Path(os.environ['BLOCKLINK_TEST_GAME_LOG'])
report=[]
def run(*args):
 return subprocess.check_output(['xdotool',*map(str,args)],text=True,timeout=10).strip()
def wait(marker):
 deadline=time.monotonic()+45
 while time.monotonic()<deadline:
  if marker in log.read_text(errors='replace'):return
  time.sleep(.1)
 raise TimeoutError('Missing server acknowledgement: '+marker)
def hold(key,seconds):
 run('keydown',key)
 try:time.sleep(seconds)
 finally:run('keyup',key)
 report.append({'input':key,'heldSeconds':seconds})

try:windows=run('search','--onlyvisible','--pid',os.environ['BLOCKLINK_TEST_GAME_PID']).splitlines()
except subprocess.CalledProcessError:windows=run('search','--onlyvisible','--name','Minecraft').splitlines()
assert windows,'Minecraft window not found'
window=windows[0]
run('windowfocus','--sync',window)
try:
 wait('BL_MOVE_READY');hold('w',1.2);wait('BL_MOVE_PASS')
 wait('BL_JUMP_READY');hold('space',.6);wait('BL_JUMP_PASS')
 wait('BL_PLACE_READY');run('key','1');time.sleep(.3);run('click',3);report.append({'input':'right mouse click'});wait('BL_PLACE_PASS')
 subprocess.run(['scrot','cloud-checks/block-placed.png'],check=True,timeout=10)
 wait('BL_BREAK_READY');run('click',1);report.append({'input':'left mouse click'});wait('BL_BREAK_PASS')
 wait('BL_ACTIONS_PASS')
 subprocess.run(['scrot','cloud-checks/gameplay.png'],check=True,timeout=10)
 print('PASS: real keyboard/mouse movement, jump, placement and break acknowledged by remote server',flush=True)
finally:
 run('keyup','w','space')
 pathlib.Path('cloud-checks/gameplay-input.json').write_text(json.dumps(report,indent=2))
