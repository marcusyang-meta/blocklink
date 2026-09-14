import {Miniflare} from 'miniflare';
import {readFile} from 'node:fs/promises';
const mf = new Miniflare({modules:true, script:await readFile(new URL('../src/worker.js',import.meta.url),'utf8'),
  compatibilityDate:'2026-07-01',host:'127.0.0.1',port:8787,
  durableObjects:{ROOMS:{className:'Room',useSQLite:true}},
  bindings:{LOBBY_HOST_KEY:'isolated-test-host-key',LOCAL_TEST:'true',MAX_ROOMS:'20',MAX_MEMBERS:'8'}});
console.log('Isolated lobby ready:',String(await mf.ready));
process.on('SIGINT',async()=>{await mf.dispose();process.exit(0)});
