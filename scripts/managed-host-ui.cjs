// UI smoke uses a simulated agent; it does not validate a real SSH deployment.
const { chromium } = require(process.env.CODEX_PRIMARY_RUNTIME_NODE_MODULES + '/playwright');
const { spawn } = require('node:child_process');
const path = require('node:path');
const assert = require('node:assert/strict');
(async () => {
 const preview = spawn(process.execPath,[path.resolve(__dirname,'../desktop/node_modules/vite/bin/vite.js'),'--host','127.0.0.1','--port','1429','--strictPort'],{cwd:path.resolve(__dirname,'../desktop'),stdio:'ignore'});
 let browser;
 try {
  for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:1429')).ok)break}catch{}await new Promise(r=>setTimeout(r,100));}
  browser=await chromium.launch({executablePath:process.env.BLOCKLINK_CHROMIUM_PATH,args:['--no-sandbox']});
  const page=await browser.newPage({viewport:{width:1360,height:1000}}),errors=[];
  page.on('pageerror',e=>errors.push(String(e)));
  await page.addInitScript(()=>{
   localStorage.setItem('blocklink.language','en');
   const id='12345678-1234-4234-8234-123456789abc';
   window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
    if(command!=='call')return null;
    switch(args.action){
     case 'status':return {instances:[],jobs:[],settings:{},platform:'Linux',store:{bytes:0,count:0}};
     case 'versions':return [];
     case 'home-summary':return {items:[]};
     case 'peer-status':return {};
     case 'lobby-status':return {sessions:[]};
     case 'managed-list':return [{id:'c'.repeat(32),name:'Fedora · Survival',deployment:{stage:'Agent connected',state:'done'}}];
     case 'managed-status':return {online:true,snapshot:{instances:[{instance:{instanceId:id,name:'Friends survival',minecraft:'1.21.1',loader:{kind:'fabric',version:'0.16.0'},runtime:{memoryMiB:4096}},running:false,installed:true,port:25565}],jobs:[]},commands:[]};
     case 'managed-command':window.lastManagedCommand=args.payload;return {...args.payload,id:args.payload.requestId,instanceId:args.payload.payload.id,status:'queued'};
     default:return {};
    }
   }};
  });
  await page.goto('http://127.0.0.1:1429');
  await page.getByRole('button',{name:'Remote servers',exact:true}).click();
  await page.getByRole('button',{name:'Fedora · Survival',exact:true}).click();
  await page.getByRole('heading',{name:'Manage server',exact:true}).waitFor();
  const start=page.getByRole('button',{name:'Start server',exact:true});
  assert(await start.isDisabled());
  await page.screenshot({path:path.resolve(__dirname,'../docs/screenshots/managed-hosts-preview.png'),fullPage:true});
  await page.getByRole('checkbox',{name:/Minecraft EULA/}).check();
  await start.click();
  const request=await page.evaluate(()=>window.lastManagedCommand);
  assert.equal(request.action,'launch');assert.equal(request.payload.eula,true);assert(request.requestId);
  await page.getByRole('button',{name:'Install or repair connection',exact:true}).click();
  await page.getByRole('heading',{name:'Automatic deployment',exact:true}).waitFor();
  const mode=page.getByRole('combobox').filter({hasText:'Docker'});
  assert.equal(await mode.count(),1);
  await mode.click();
  await page.getByRole('option',{name:'systemd',exact:true}).click();
  assert.equal(await page.getByRole('combobox').filter({hasText:'systemd'}).count(),1);
  await page.getByRole('combobox').filter({hasText:'systemd'}).click();
  await page.getByRole('option',{name:'Docker',exact:true}).click();
  await page.getByRole('heading',{name:'Automatic deployment',exact:true}).scrollIntoViewIfNeeded();
  await page.screenshot({path:path.resolve(__dirname,'../docs/screenshots/docker-deployment-preview.png')});
  assert.deepEqual(errors,[]);
  console.log('Managed host UI smoke passed: render, EULA gate and command submission.');
 } finally {if(browser)await browser.close();preview.kill();}
})().catch(e=>{console.error(e);process.exitCode=1});
