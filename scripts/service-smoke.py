"""Exercise the built native host on every CI platform without signing in."""
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.request
import urllib.error

binary=pathlib.Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix='blocklink-smoke-') as temp:
    root=pathlib.Path(temp)
    process=subprocess.Popen([str(binary),'--service',str(root)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if process.poll() is not None: raise RuntimeError('Native service exited before startup')
            try:
                config=json.loads((root/'service.json').read_text())
                break
            except (FileNotFoundError,json.JSONDecodeError): time.sleep(.1)
        else: raise RuntimeError('Service startup timed out')
        url=f'http://127.0.0.1:{config["port"]}/rpc'
        payload=json.dumps({'action':'status','payload':{}}).encode()
        try:
            urllib.request.urlopen(urllib.request.Request(url,data=payload),timeout=5)
            raise AssertionError('Unauthenticated request was accepted')
        except urllib.error.HTTPError as error: assert error.code==403
        request=urllib.request.Request(url,data=payload,headers={'Authorization':'Bearer '+config['token']})
        response=json.load(urllib.request.urlopen(request,timeout=10))
        assert response['ok'] is True
        assert response['value']['instances']==[]
        print('Native host starts, returns status, and rejects unauthenticated requests.')
        request=urllib.request.Request(url,data=json.dumps({'action':'prepare-app-update','payload':{}}).encode(),headers={'Authorization':'Bearer '+config['token']})
        assert json.load(urllib.request.urlopen(request,timeout=10))['value']['ready'] is True
        assert process.wait(timeout=10)==0
        print('Idle native host shuts down cleanly for app replacement.')
    finally:
        if process.poll() is None: process.terminate()
        try: process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)
