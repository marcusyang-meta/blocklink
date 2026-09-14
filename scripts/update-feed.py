"""Assemble the updater feed only after all platform payloads are available."""
import json,pathlib,re,sys,urllib.parse
artifacts=pathlib.Path(sys.argv[1])
tag=sys.argv[2]
if not re.fullmatch(r'v[0-9A-Za-z._-]+',tag):raise ValueError('Invalid release tag')
version=None
platforms={}
for metadata in artifacts.rglob('update-*.json'):
    item=json.loads(metadata.read_text())
    if version is not None and version!=item['version']:raise ValueError('Mixed app versions')
    version=item['version']
    payload=metadata.parent/item['file']
    if payload.parent!=metadata.parent or not payload.is_file():raise ValueError('Missing updater payload')
    if not item['signature'] or item['target'] in platforms:raise ValueError('Duplicate or unsigned payload')
    platforms[item['target']]={'signature':item['signature'],'url':f'https://github.com/marcusyang-meta/blocklink/releases/download/{tag}/{urllib.parse.quote(payload.name)}'}
if set(platforms)!={'windows-x86_64','darwin-aarch64','darwin-x86_64','linux-x86_64'}:raise ValueError('All four signed builds are required before publishing an update')
pathlib.Path('latest.json').write_text(json.dumps({'version':version,'notes':f'Blocklink {tag}. See GitHub release notes for changes.','platforms':platforms},indent=2)+'\n')
