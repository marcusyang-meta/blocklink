import {t,getLocale,ServiceMessage} from './i18n';
import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
export function Shaders({item,jobs,onRefresh}:{item:Json;jobs:Json[];onRefresh:()=>void}){
 const [data,setData]=useState<Json>({packs:[],history:[],warnings:[]}),[error,setError]=useState(''),[pending,setPending]=useState(false);
 const active=jobs.find(j=>j.instanceId===item.instance.instanceId&&['queued','running'].includes(j.status));
 const locked=pending||!!active||item.running;
 const load=()=>invoke<Json>('call',{action:'shader-state',payload:{id:item.instance.instanceId}}).then(setData).catch(e=>setError(String(e)));
 useEffect(()=>{void load()},[item.instance.instanceId,JSON.stringify(jobs)]);
 const run=async(action:string,payload:Json)=>{setPending(true);setError('');try{await invoke('call',{action,payload:{id:item.instance.instanceId,...payload}});onRefresh();await load()}catch(e){setError(String(e))}finally{setPending(false)}};
 return <div className="mod-update-panel"><div className="section-heading"><div><h2>{t("光影")}</h2><p>{t("管理实例 shaderpacks 目录中的 ZIP 光影包。当前支持 Iris；把光影 ZIP 放入该目录后刷新即可识别。")}</p></div><button className="button" onClick={()=>void load()}>{t("刷新光影包")}</button></div>
 {!data.iris&&<div className="notice">{t("请先在 Mods 中添加适配当前游戏的 Iris，它的必需依赖会一起安装。")}</div>}
 {error&&<div className="alert">{<ServiceMessage text={error}/>}</div>}{active&&<div className="progress-panel">{<ServiceMessage text={active.message} progress/>}</div>}
 <div className="panel">{!data.packs.length?<div className="empty">{t("还没有本地光影包")}</div>:data.packs.map((p:Json)=><div className="job-row" key={p.file}><div><strong>{p.file}</strong><p>{p.active&&data.enabled?t("正在使用"):t("未启用")}</p>{p.warning&&<p className="hint">{<ServiceMessage text={p.warning}/>}</p>}</div><div className="actions"><button className="button" disabled={locked} onClick={()=>run('shader-check',{file:p.file})}>{t("自动适配")}</button><button className="button" disabled={locked||!data.iris} onClick={()=>run('shader-select',{file:p.file,enabled:!(p.active&&data.enabled)})}>{p.active&&data.enabled?t("停用"):t("启用")}</button></div></div>)}</div>
 {data.plan?.ready&&<section className="panel settings-card"><h2>{t("光影方案已准备")}</h2><p>{data.plan.from} → {data.plan.to}</p>{data.plan.reason&&<p>{<ServiceMessage text={data.plan.reason}/>}</p>}<p>{t("已核对同项目来源、文件哈希和当前已知兼容问题。保留原包与 Iris 设置快照；新版本使用自己的光影参数，显卡上的实际编译结果仍需启动确认。")}</p><button className="button primary" disabled={locked||!data.iris} onClick={()=>run('shader-apply',{planId:data.plan.id})}>{t("应用并启用兼容光影")}</button></section>}
 {!!data.warnings.length&&<section className="panel settings-card"><h2>{t("上次运行的光影警告")}</h2><p>{t("更换光影后需重新启动验证，旧日志不会被删除。")}</p>{data.warnings.map((w:string)=><p className="hint" key={w}>{<ServiceMessage text={w}/>}</p>)}</section>}
 {!!data.history.length&&<><h2>{t("光影设置快照")}</h2><div className="panel">{data.history.map((h:Json)=><div className="job-row" key={h.id}><div><strong>{h.file||t("未选择光影")}</strong><p>{new Date(h.createdAt*1000).toLocaleString(getLocale())}</p></div><button className="button" disabled={locked} onClick={()=>run('shader-restore',{snapshotId:h.id})}>{t("恢复设置")}</button></div>)}</div></>}
 </div>;
}
