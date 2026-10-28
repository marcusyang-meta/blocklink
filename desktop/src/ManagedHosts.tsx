import React,{useEffect,useRef,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t,useLocale,ServiceMessage} from './i18n';
import {Select} from './Select';
import './managed-hosts.css';
type Json=Record<string,any>;
const api=(action:string,payload:Json={})=>invoke<any>('call',{action,payload});
export function ManagedHosts(){
 useLocale();
 const [hosts,setHosts]=useState<Json[]>([]),[selected,setSelected]=useState(''),[state,setState]=useState<Json|null>(null);
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[adding,setAdding]=useState(false),[deploying,setDeploying]=useState(false),[creating,setCreating]=useState(false),[revoke,setRevoke]=useState(false);
 const [name,setName]=useState(''),[connection,setConnection]=useState({host:'',port:22,username:'root',password:'',privateKey:'',passphrase:'',fingerprint:'',releaseTag:'v0.1.4',deploymentMode:'docker'});
 const [confirmed,setConfirmed]=useState(false),[instanceId,setInstanceId]=useState(''),[command,setCommand]=useState(''),[eula,setEula]=useState(false);
 const [create,setCreate]=useState({name:'',minecraft:'1.21.1',loader:'vanilla',memory:4096,port:25565});
 const [mod,setMod]=useState(''),[memory,setMemory]=useState(4096),[port,setPort]=useState(25565),[editor,setEditor]=useState<Json|null>(null);
 const pending=useRef<Json|null>(null);const [retry,setRetry]=useState(false);
 const loadHosts=async()=>{const list=await api('managed-list');setHosts(list);return list as Json[]};
 useEffect(()=>{void loadHosts().catch(e=>setError(String(e)))},[]);
 useEffect(()=>{
  setState(null);setInstanceId('');setEditor(null);pending.current=null;setRetry(false);setError('');if(!selected)return;
  let active=true,working=false;
  const refresh=async()=>{if(working)return;working=true;try{const v=await api('managed-status',{hostId:selected});if(active)setState(v);await loadHosts()}catch(e){if(active)setError(String(e))}finally{working=false}};
  void refresh();const timer=setInterval(refresh,4000);return()=>{active=false;clearInterval(timer)};
 },[selected]);
 const perform=async(fn:()=>Promise<any>)=>{setBusy(true);setError('');try{return await fn()}catch(e){setError(String(e));return null}finally{setBusy(false)}};
 const transmit=async(request:Json)=>{setBusy(true);setError('');try{const accepted=await api('managed-command',request);setState(old=>old?{...old,commands:[...(old.commands||[]).filter((c:Json)=>c.id!==accepted.id),accepted]}:old);pending.current=null;setRetry(false)}catch(e){setError(String(e));setRetry(true)}finally{setBusy(false)}};
 const send=async(action:string,payload:Json={})=>{const request={hostId:selected,requestId:crypto.randomUUID(),action,payload};pending.current=request;await transmit(request)};
 const host=hosts.find(h=>h.id===selected),instances=(state?.snapshot?.instances||[]) as Json[],jobs=(state?.snapshot?.jobs||[]) as Json[],commands=(state?.commands||[]) as Json[];
 const current=instances.find(i=>i.instance.instanceId===instanceId),localBusy=busy||retry;
 const instanceBusy=jobs.some(j=>j.instanceId===instanceId&&['queued','running'].includes(j.status))||commands.some(c=>c.instanceId===instanceId&&['queued','dispatched'].includes(c.status));
 const results=commands.filter(c=>c.instanceId===instanceId&&c.status==='done');
 const last=(action:string)=>results.filter(c=>c.action===action).slice(-1)[0]?.result;
 const backups=last('world-backups')?.items||[],mods=last('server-mods')||[],listing=last('server-files');
 const fileResult=results.filter(c=>['server-file-read','server-file-write'].includes(c.action)&&c.result?.sha256).slice(-1)[0];
 useEffect(()=>{if(!instanceId&&instances.length)setInstanceId(instances[0].instance.instanceId)},[instances.length,instanceId]);
 useEffect(()=>{setEditor(null);setEula(false);if(current){setMemory(current.instance.runtime.memoryMiB);setPort(current.port)}},[instanceId]);
 useEffect(()=>{if(fileResult){if(fileResult.action==='server-file-read')setEditor(fileResult.result);else setEditor(old=>old?.path===fileResult.result.path?{...old,sha256:fileResult.result.sha256}:old)}},[fileResult?.id]);
 return <section className="managed-hosts">
  <div className="section-heading"><div><h1>{t("远程服务器")}</h1><p>{t("添加自己的 Linux 主机，部署后在这里管理。")}</p></div><button className="button primary" onClick={()=>setAdding(!adding)}>{t("添加主机")}</button></div>
  <p className="hint">{t("当前预览支持 x86_64 Linux：Docker 或 systemd 部署。主机需要能连接 SSH，并能向外访问 HTTPS。")}</p>
  {error&&<div role="alert" className="alert"><ServiceMessage text={error}/></div>}
  {retry&&<div className="hint"><span>{t("提交结果尚未确认。重试会使用同一任务 ID，避免重复操作。")}</span><button className="button" disabled={busy} onClick={()=>pending.current&&transmit(pending.current)}>{t("重试提交")}</button></div>}
  {adding&&<form className="panel settings-card" onSubmit={e=>{e.preventDefault();void perform(async()=>{let h;try{h=await api('managed-register',{name})}finally{const list=await loadHosts();if(list.length)setSelected(list[list.length-1].id)}setSelected(h.id);setAdding(false);setDeploying(true)})}}>
   <h2>{t("添加主机")}</h2><label>{t("名称")}<input required maxLength={80} value={name} onChange={e=>setName(e.target.value)}/></label><p>{t("使用启动器设置中保存的大厅地址和开房凭据注册管理身份。")}</p><button className="button primary" disabled={busy}>{t("继续")}</button>
  </form>}
  <div className="host-tabs" role="group" aria-label={t("选择主机")}>{hosts.map(h=><button className={'button '+(h.id===selected?'primary':'')} key={h.id} onClick={()=>{setSelected(h.id);setDeploying(false);setRevoke(false);setConnection(c=>({...c,password:'',privateKey:'',passphrase:'',fingerprint:''}));setConfirmed(false)}}>{h.name}</button>)}</div>
  {!hosts.length&&!adding&&<div className="empty"><h2>{t("还没有远程主机")}</h2><p>{t("先添加一台主机，再创建 Minecraft 服务器。")}</p></div>}
  {host&&<>
   <div className="panel settings-card"><div className="section-heading"><h2>{host.name}</h2><span className="tag">{state?.online?t("在线"):t("等待连接")}</span></div><p>{host.deployment&&<ServiceMessage text={host.deployment.stage}/>}</p><button className="button" onClick={()=>setDeploying(!deploying)}>{t("安装或修复连接")}</button><button className="text-link" onClick={()=>setRevoke(!revoke)}>{t("撤销管理权限")}</button>{revoke&&<div className="hint"><span>{t("撤销后将无法继续管理，已经运行的游戏服务器不会停止。")}</span><button className="button danger" disabled={busy} onClick={()=>perform(async()=>{await api('managed-revoke',{hostId:selected});setState(null);setRevoke(false)})}>{t("确认撤销")}</button></div>}</div>
   {deploying&&<form className="panel settings-card" onSubmit={e=>{e.preventDefault();void perform(async()=>{await api('managed-deploy',{hostId:selected,...connection});setConnection({...connection,password:'',privateKey:'',passphrase:''});setDeploying(false);await loadHosts()})}}>
    <h2>{t("自动部署")}</h2><label>{t("部署方式")}<Select value={connection.deploymentMode} onChange={e=>setConnection({...connection,deploymentMode:e.target.value})}><option value="docker">Docker</option><option value="systemd">systemd</option></Select></label><p>{t("Docker 方式需要预先安装 Docker。数据独立持久保存，使用主机网络，不会接管现有 Crafty 存档。")}</p><div className="form-grid"><label>{t("主机地址")}<input required autoComplete="off" value={connection.host} onChange={e=>{setConnection({...connection,host:e.target.value,fingerprint:''});setConfirmed(false)}}/></label><label>{t("SSH 端口")}<input type="number" required min={1} max={65535} value={connection.port} onChange={e=>{setConnection({...connection,port:Number(e.target.value),fingerprint:''});setConfirmed(false)}}/></label></div>
    <label>{t("SSH 用户名")}<input required value={connection.username} onChange={e=>setConnection({...connection,username:e.target.value})}/></label><p>{t("使用 root，或具有免密码 sudo 权限的专用部署用户。")}</p>
    <button type="button" className="button" disabled={busy||!connection.host} onClick={()=>perform(async()=>{const r=await api('managed-probe',{host:connection.host,port:connection.port});setConnection({...connection,fingerprint:r.fingerprint});setConfirmed(false)})}>{t("检查主机身份")}</button>
    {connection.fingerprint&&<><code className="host-fingerprint">{connection.fingerprint}</code><label className="checkbox"><input type="checkbox" checked={confirmed} onChange={e=>setConfirmed(e.target.checked)}/>{t("我已核对主机指纹，信任这台服务器")}</label></>}
    <label>{t("SSH 密码")}<input type="password" autoComplete="new-password" value={connection.password} onChange={e=>setConnection({...connection,password:e.target.value})}/></label>
    <label>{t("或上传 SSH 私钥")}<input type="file" onChange={e=>{const f=e.target.files?.[0];if(f){if(f.size>32768){setError(t("私钥文件过大"));return}void f.text().then(privateKey=>setConnection(c=>({...c,privateKey,password:''}))).catch(e=>setError(String(e)))}}}/></label>
    <label>{t("私钥口令（如有）")}<input type="password" autoComplete="new-password" value={connection.passphrase} onChange={e=>setConnection({...connection,passphrase:e.target.value})}/></label><label>{t("服务发行版本")}<input required value={connection.releaseTag} onChange={e=>setConnection({...connection,releaseTag:e.target.value})}/></label>
    <p>{t("首次部署凭据不长期保存。关闭 App 后服务器仍继续运行。发行版本必须包含无界面服务附件。")}</p><button className="button primary" disabled={busy||!confirmed||!(connection.password||connection.privateKey)}>{t("部署到这台主机")}</button>
   </form>}
   {state?.online&&<>
    <button className="button primary" onClick={()=>setCreating(!creating)}>{t("新建服务器")}</button>
    {(creating||!instances.length)&&<form className="panel settings-card" onSubmit={e=>{e.preventDefault();void send('create',{...create,server:true,install:true})}}><h2>{t("新建服务器")}</h2><div className="form-grid">
     <label>{t("名称")}<input required maxLength={80} value={create.name} onChange={e=>setCreate({...create,name:e.target.value})}/></label><label>{t("Minecraft 版本")}<input required value={create.minecraft} onChange={e=>setCreate({...create,minecraft:e.target.value})}/></label>
     <label>{t("模组支持")}<Select value={create.loader} onChange={e=>setCreate({...create,loader:e.target.value})}>{['vanilla','fabric','neoforge','forge','quilt'].map(l=><option key={l}>{l}</option>)}</Select></label>
     <label>{t("内存（MB）")}<input type="number" min={512} max={131072} value={create.memory} onChange={e=>setCreate({...create,memory:Number(e.target.value)})}/></label><label>{t("服务器端口")}<input type="number" min={1024} max={65535} value={create.port} onChange={e=>setCreate({...create,port:Number(e.target.value)})}/></label>
    </div><button className="button primary" disabled={localBusy||jobs.some(j=>j.action==='create'&&['queued','running'].includes(j.status))||commands.some(c=>c.action==='create'&&['queued','dispatched'].includes(c.status))}>{t("创建并安装")}</button></form>}
    <section className="panel settings-card"><h2>{t("管理服务器")}</h2><label>{t("选择服务器")}<Select value={instanceId} onChange={e=>setInstanceId(e.target.value)}><option value="">{t("请选择")}</option>{instances.map(i=><option key={i.instance.instanceId} value={i.instance.instanceId}>{i.instance.name}</option>)}</Select></label>
    {current&&<><p>{current.instance.minecraft} · {current.running?t("正在运行"):t("已停止")}</p><label className="checkbox"><input type="checkbox" checked={eula} onChange={e=>setEula(e.target.checked)}/>{t("我已阅读并同意 Minecraft EULA")}</label><button className="text-link" onClick={()=>invoke('open_link',{url:'https://www.minecraft.net/eula'}).catch(e=>setError(String(e)))}>{t("阅读 Minecraft EULA ")}</button>
     <div className="actions"><button className="button primary" disabled={localBusy||instanceBusy||current.running||!eula} onClick={()=>send('launch',{id:instanceId,eula:true})}>{t("启动服务器")}</button><button className="button" disabled={localBusy||!current.running} onClick={()=>send('stop',{id:instanceId})}>{t("停止服务器")}</button><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('install',{id:instanceId})}>{t("检查并修复安装")}</button><button className="button" disabled={localBusy} onClick={()=>send('logs',{id:instanceId})}>{t("刷新日志")}</button></div>
     <pre className="console">{last('logs')?.text||t("等待日志…")}</pre><form className="console-input" onSubmit={e=>{e.preventDefault();void send('console',{id:instanceId,command});setCommand('')}}><input aria-label={t("服务器命令")} value={command} onChange={e=>setCommand(e.target.value)} maxLength={2048} placeholder="list"/><button className="button" disabled={localBusy||!current.running||!command.trim()}>{t("发送")}</button></form>
     <h3>{t("服务器设置")}</h3><form onSubmit={e=>{e.preventDefault();void send('configure',{id:instanceId,memory,port})}}><div className="form-grid"><label>{t("内存（MB）")}<input type="number" min={512} max={131072} value={memory} onChange={e=>setMemory(Number(e.target.value))}/></label><label>{t("服务器端口")}<input type="number" min={1024} max={65535} value={port} onChange={e=>setPort(Number(e.target.value))}/></label></div><button className="button" disabled={localBusy||current.running||instanceBusy}>{t("保存设置")}</button></form>
     <h3>{t("配置文件")}</h3><button className="button" disabled={localBusy} onClick={()=>send('server-files',{id:instanceId})}>{t("浏览配置文件")}</button>
     {listing&&<div className="host-files"><p>{listing.path||'/'}</p><button className="text-link" disabled={localBusy} onClick={()=>send('server-files',{id:instanceId,path:listing.path.split('/').slice(0,-1).join('/')})}>{t("上级目录")}</button>{listing.entries.map((f:Json)=><button key={f.path} className="button" disabled={localBusy} onClick={()=>send(f.directory?'server-files':'server-file-read',{id:instanceId,path:f.path})}>{f.name}{f.directory?'/':''}</button>)}</div>}
     {editor&&<form onSubmit={e=>{e.preventDefault();void send('server-file-write',{id:instanceId,path:editor.path,text:editor.text,expectedSha256:editor.sha256})}}><label>{editor.path}<textarea rows={10} maxLength={16000} value={editor.text} onChange={e=>setEditor({...editor,text:e.target.value})}/></label><p>{t("保存前会备份原文件；文件被其他操作修改时需要重新打开。")}</p><button className="button" disabled={localBusy||current.running||instanceBusy}>{t("保存文件")}</button></form>}
     <h3>{t("模组与备份")}</h3><form onSubmit={e=>{e.preventDefault();void send('mod-add',{id:instanceId,project:mod})}}><label>{t("Modrinth 项目 ID 或短名称")}<input required value={mod} onChange={e=>setMod(e.target.value)}/></label><button className="button" disabled={localBusy||current.running||instanceBusy}>{t("安装模组")}</button></form>
     <div className="actions"><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('publish',{id:instanceId})}>{t("发布环境")}</button><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('world-backup',{id:instanceId})}>{t("创建世界备份")}</button><button className="button" disabled={localBusy} onClick={()=>send('world-backups',{id:instanceId})}>{t("查看备份")}</button><button className="button" disabled={localBusy} onClick={()=>send('server-mods',{id:instanceId})}>{t("查看已安装模组")}</button></div>
     {Array.isArray(mods)&&mods.map((m:Json)=><div className="host-task" key={m.modId}><span>{m.modId}</span><span>{m.version}</span><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('mod-remove',{id:instanceId,modId:m.modId})}>{t("移除模组")}</button></div>)}
     {backups.map((b:Json)=><div className="host-task" key={b.id}><strong>{b.name}</strong><span>{new Date(b.createdAt*1000).toLocaleDateString()}</span><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('world-backup-restore',{id:instanceId,backupId:b.id})}>{t("恢复为独立世界")}</button></div>)}
     {jobs.filter(j=>j.instanceId===instanceId&&j.action==='world-backup-restore'&&j.status==='done'&&j.result?.folder).map(j=><div className="host-task" key={j.id}><span>{j.result.folder}</span><button className="button" disabled={localBusy||current.running||instanceBusy} onClick={()=>send('world-activate',{id:instanceId,folder:j.result.folder})}>{t("切换到恢复的世界")}</button></div>)}
    </>}
    </section>
   </>}
   {state?.snapshot?.truncated&&<p role="status">{t("主机数据超过当前页面容量，部分结果未显示。")}</p>}
   <section className="panel settings-card"><h2>{t("任务与结果")}</h2><p>{t("已接收表示服务器收到操作；安装是否完成请看下方后台任务。")}</p>{jobs.map(j=><div className="host-task" key={j.id}><strong>{j.action}</strong><span>{j.status}</span><ServiceMessage text={j.message}/></div>)}{commands.slice().reverse().slice(0,15).map(c=><details key={c.id}><summary>{c.action} · {c.status==='done'?t("已接收"):c.status}</summary>{c.error&&<ServiceMessage text={c.error}/>}<pre>{JSON.stringify(c.result,null,2)}</pre></details>)}</section>
  </>}
 </section>
}
