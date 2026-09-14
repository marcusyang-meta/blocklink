import {test} from 'node:test';
import assert from 'node:assert/strict';
import worker from '../src/worker.js';
test('product assets are separate from invitation and private room routes',async()=>{
 const seen=[];const env={ASSETS:{fetch:async request=>{seen.push(new URL(request.url).pathname);return new Response('asset',{headers:{'Content-Type':'text/html'}})}}};
 for(const [path,target] of [['/','/'],['/privacy','/privacy'],['/en','/en'],['/en/','/en/'],['/en/privacy','/en/privacy'],['/assets/language.js','/assets/language.js'],['/assets/site.css','/assets/site.css'],['/downloads/Blocklink.exe','/downloads/Blocklink.exe']]){
  const r=await worker.fetch(new Request('https://example.test'+path),env);assert.equal(r.status,200);assert.equal(seen.at(-1),target);assert.equal(r.headers.get('Referrer-Policy'),'no-referrer');
  if(path.endsWith('.exe'))assert.match(r.headers.get('Content-Disposition'),/attachment/);
 }
 const before=seen.length;const invite=await worker.fetch(new Request('https://example.test/invite/'+'a'.repeat(32)),env);assert.match(await invite.text(),/复制邀请链接/);assert.equal(seen.length,before);
 const privateRoute=await worker.fetch(new Request('https://example.test/api/rooms',{method:'POST',body:'{}'}),env);assert.equal(privateRoute.status,401);assert.equal(seen.length,before);
});
test('updater feed is public and briefly cached without exposing other update paths',async()=>{
 const env={ASSETS:{fetch:async()=>Response.json({version:'0.1.1',platforms:{}})}};
 const response=await worker.fetch(new Request('https://example.test/updates/latest.json'),env);
 assert.equal(response.status,200);assert.equal((await response.json()).version,'0.1.1');assert.equal(response.headers.get('Cache-Control'),'public, max-age=300');
 assert.equal((await worker.fetch(new Request('https://example.test/updates/private.key'),env)).status,404);
});
