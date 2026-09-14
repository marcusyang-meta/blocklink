"""Publish all tested native artifacts and a complete signed feed for an existing release."""
import json,os,pathlib,re,subprocess,sys,tempfile,urllib.parse
def gh(*args):return subprocess.check_output(['gh',*args],text=True)
def api(path):return json.loads(gh('api',path))
repo=os.environ['GH_REPO'];run=sys.argv[1];sha=sys.argv[2]
assert re.fullmatch(r'[0-9]+',run) and re.fullmatch(r'[0-9a-f]{40}',sha)
info=api(f'repos/{repo}/actions/runs/{run}')
assert info['conclusion']=='success' and info['head_sha']==sha and info['name']=='Build Windows, macOS and Linux'
release=None
for candidate in api(f'repos/{repo}/releases?per_page=30'):
    tag=candidate['tag_name']
    if candidate['draft'] or not re.fullmatch(r'v[0-9A-Za-z._-]+',tag):continue
    if api(f'repos/{repo}/commits/{tag}')['sha']==sha:release=candidate;break
if release is None:
    print('No published release targets this build; artifacts stay in the workflow.');sys.exit(0)
tag=release['tag_name'];existing={a['name'] for a in release['assets']}
with tempfile.TemporaryDirectory() as temporary:
    root=pathlib.Path(temporary);gh('run','download',run,'--dir',str(root))
    metadata=[json.loads(p.read_text()) for p in root.rglob('update-*.json')]
    expected={'windows-x86_64','darwin-aarch64','darwin-x86_64','linux-x86_64'}
    assert {x['target'] for x in metadata}==expected and len(metadata)==4,'All signed platforms are required'
    assert len({x['version'] for x in metadata})==1,'Mixed versions'
    for file in sorted(root.rglob('*')):
        if not file.is_file() or file.suffix=='.json':continue
        if not (file.name.startswith('Blocklink-update-') or file.name.startswith('Blocklink-') and file.suffix in ('.zip','.sha256') or file.name.startswith('Blocklink_') and file.suffix in ('.dmg','.deb','.AppImage')):continue
        if file.name in existing:continue
        gh('release','upload',tag,str(file));existing.add(file.name);print('Published',file.name)
    # An earlier local Windows build may already be published. Use its actual immutable
    # signature rather than advertising a signature from a later CI rebuild of that version.
    signatures=root/'published-signatures';signatures.mkdir()
    platforms={}
    for item in metadata:
        name=item['file'];assert name in existing and name+'.sig' in existing
        gh('release','download',tag,'--pattern',name+'.sig','--dir',str(signatures))
        platforms[item['target']]={'url':f'https://github.com/{repo}/releases/download/{tag}/{urllib.parse.quote(name)}','signature':(signatures/(name+'.sig')).read_text().strip()}
    feed={'version':metadata[0]['version'],'notes':f'Blocklink {tag}. See GitHub release notes for changes.','platforms':platforms}
    path=root/'latest.json';path.write_text(json.dumps(feed,indent=2)+'\n')
    if 'latest.json' not in existing:gh('release','upload',tag,str(path))
    body=release.get('body','')
    body=re.sub(r'Windows packages below have passed native smoke tests\. Updated macOS and Linux packages.*?successful checks\.', 'All four native builds passed tests and native service smoke checks. Windows, Apple Silicon, Intel Mac and Linux packages are attached below. macOS/Linux gameplay and graphics still need device testing.',body,flags=re.S)
    notes=root/'release.md';notes.write_text(body);gh('release','edit',tag,'--notes-file',str(notes))
print('Complete signed updater feed published:',tag)
