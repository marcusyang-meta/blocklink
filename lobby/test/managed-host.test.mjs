import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {Miniflare} from 'miniflare';
test('managed host authorization, durable delivery, idempotency and revocation',async()=>{
 const mf=new Miniflare({modules:true,script:await readFile(new URL('../src/worker.js',import.meta.url),'utf8'),compatibilityDate:'2026-07-01',durableObjects:{HOSTS:{className:'ManagedHost',useSQLite:true},ROOMS:{className:'Room',useSQLite:true}},bindings:{LOBBY_HOST_KEY:'registration'}});
 try{
  const owner='a'.repeat(64),agent='b'.repeat(64),id='c'.repeat(32),path='/api/hosts/'+id;
  const call=(path,token,payload)=>mf.dispatchFetch('http://lobby'+path,{method:payload===undefined?'GET':'POST',headers:{Authorization:'Bearer '+token},body:payload===undefined?undefined:JSON.stringify(payload)});
  const enrollment={id,name:'My machine',ownerToken:owner,agentToken:agent};
  assert.equal((await call('/api/hosts','bad',enrollment)).status,401);assert.equal((await call('/api/hosts','registration',enrollment)).status,201);assert.equal((await call('/api/hosts','registration',enrollment)).status,200);
  assert.equal((await call(path,'bad')).status,401);assert.equal((await call(path,agent)).status,403);assert.equal((await call(path+'/poll',owner,{})).status,403);assert.equal((await call('/init',owner,{})).status,404);
  const cmd={id:crypto.randomUUID(),action:'create',payload:{name:'Survival',server:true}};
  const r=await Promise.all([call(path+'/commands',owner,cmd),call(path+'/commands',owner,cmd)]);assert.deepEqual(r.map(r=>r.status).sort(),[200,202]);
  assert.equal((await call(path+'/commands',owner,{...cmd,action:'stop'})).status,409);assert.equal((await call(path+'/commands',owner,{...cmd,id:crypto.randomUUID(),action:'shell'})).status,400);
  assert.equal((await(await call(path+'/poll',agent,{snapshot:{instances:[]}})).json()).command.id,cmd.id);assert.equal((await(await call(path+'/poll',agent,{})).json()).command.id,cmd.id);
  const completion={id:cmd.id,ok:true,value:{jobId:'service-job'}};assert.equal((await(await call(path+'/poll',agent,{completion})).json()).command,null);assert.equal((await call(path+'/poll',agent,{completion})).status,200);
  const state=await(await call(path,owner)).json();assert.equal(state.online,true);assert.equal(state.commands.length,1);assert.equal(state.commands[0].status,'done');assert.equal(state.commands[0].result.jobId,'service-job');
  for(const key of ['ownerHash','agentHash','ownerToken','agentToken','payload'])assert.equal(JSON.stringify(state).includes('"'+key+'"'),false);
  assert.equal((await call(path+'/revoke',agent,{})).status,403);assert.equal((await call(path+'/revoke',owner,{})).status,200);assert.equal((await call(path+'/poll',agent,{})).status,410);
 }finally{await mf.dispose();}
});
