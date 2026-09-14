"""Create platform-specific, signed updater payloads from already tested builds."""
import json, os, pathlib, shutil, subprocess, sys, tarfile
release=pathlib.Path(sys.argv[1]).resolve()
label=sys.argv[2]
out=pathlib.Path('dist-packages').resolve()
out.mkdir(exist_ok=True)
if not (os.environ.get('TAURI_SIGNING_PRIVATE_KEY') or os.environ.get('TAURI_SIGNING_PRIVATE_KEY_PATH')):
    print('No signing key: portable packages remain available; no updater payload published.')
    sys.exit(0)
if label=='Windows-x64':
    target='windows-x86_64'
    payload=out/'Blocklink-update-Windows-x64.exe'
    shutil.copyfile(release/'blocklink-desktop.exe',payload)
elif label.startswith('macOS'):
    target='darwin-aarch64' if label.endswith('Apple-Silicon') else 'darwin-x86_64'
    payload=out/f'Blocklink-update-{label}.app.tar.gz'
    with tarfile.open(payload,'w:gz') as archive:
        archive.add(release/'bundle/macos/Blocklink.app',arcname='Blocklink.app')
else:
    target='linux-x86_64'
    payload=out/'Blocklink-update-Linux-x64.AppImage'
    images=list((release/'bundle/appimage').glob('*.AppImage'))
    if len(images)!=1: raise RuntimeError('Expected one AppImage')
    shutil.copyfile(images[0],payload)
signing_env=dict(os.environ)
signing_env.setdefault('TAURI_SIGNING_PRIVATE_KEY_PASSWORD','')
subprocess.run(['node','desktop/node_modules/@tauri-apps/cli/tauri.js','signer','sign',str(payload)],check=True,env=signing_env,timeout=120)
version=json.loads(pathlib.Path('desktop/src-tauri/tauri.conf.json').read_text())['version']
(out/f'update-{target}.json').write_text(json.dumps({'version':version,'target':target,'file':payload.name,'signature':payload.with_name(payload.name+'.sig').read_text().strip()},indent=2)+'\n')
