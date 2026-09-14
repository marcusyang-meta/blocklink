"""Package existing native builds on the OS that built them."""
import hashlib
import pathlib
import shutil
import subprocess
import sys
import tempfile
import zipfile
release=pathlib.Path(sys.argv[1]).resolve()
label=sys.argv[2]
out=pathlib.Path('dist-packages').resolve()
out.mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix='blocklink-package-') as temporary:
    stage=pathlib.Path(temporary)/'Blocklink'
    stage.mkdir()
    shutil.copy('LICENSE',stage/'LICENSE.txt')
    shutil.copy('PLATFORMS.md',stage/'README.txt')
    shutil.copytree('desktop/src-tauri/notices',stage/'notices')
    if sys.platform=='win32':
        shutil.copy(release/'blocklink-desktop.exe',stage/'Blocklink.exe')
    elif sys.platform=='darwin':
        app=release/'bundle/macos/Blocklink.app'
        if not app.is_dir(): raise RuntimeError('macOS app bundle missing')
        subprocess.run(['ditto',str(app),str(stage/'Blocklink.app')],check=True)
    else:
        images=list((release/'bundle/appimage').glob('*.AppImage'))
        if len(images)!=1: raise RuntimeError('Expected one AppImage')
        shutil.copy2(images[0],stage/'Blocklink.AppImage')
        (stage/'Blocklink.AppImage').chmod(0o755)
    archive=out/f'Blocklink-{label}.zip'
    if sys.platform=='darwin':
        subprocess.run(['ditto','-c','-k','--sequesterRsrc','--keepParent',str(stage),str(archive)],check=True)
    else:
        with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,strict_timestamps=False) as zipped:
            for item in stage.rglob('*'):
                if item.is_file(): zipped.write(item,item.relative_to(stage.parent))
    checksum=hashlib.sha256(archive.read_bytes()).hexdigest()
    (out/f'{archive.name}.sha256').write_text(f'{checksum}  {archive.name}\n')
    print(archive.name)
