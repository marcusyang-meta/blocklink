import {t,getLocale,ServiceMessage} from './i18n';
import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
export function TrashPanel({item,jobs,onDeleted,onError}:{item?:Json;jobs:Json[];onDeleted:()=>void;onError:(message:string)=>void}) {
 const [entries,setEntries]=useState<Json[]>([]),[name,setName]=useState(''),[job,setJob]=useState(''),[pending,setPending]=useState(false);
 const load=()=>invoke<Json[]>('call',{action:'trash-list',payload:{}}).then(setEntries).catch(e=>onError(String(e)));
 useEffect(()=>{if(!item)void load()},[]);
 useEffect(()=>{if(!job)return;const result=jobs.find(j=>j.id===job);if(result?.status==='done'){setJob('');setPending(false);if(item)onDeleted();else void load()}else if(result?.status==='error'){setJob('');setPending(false);onError(result.message)}},[jobs,job]);
 const act=async(action:string,payload:Json)=>{setPending(true);try{const r=await invoke<Json>('call',{action,payload});setJob(r.jobId)}catch(e){setPending(false);onError(String(e))}};
 return item?<><h2>{t("永久删除")}{item.config.server?t("服务器"):t("实例")}：{item.instance.name}</h2><p>{t("将彻底删除此实例的游戏文件、存档、配置、光影和本地备份，不经过回收站，无法恢复。")}</p><p>{t("联机房间会关闭，本机绑定会解除。其他游戏和共享下载缓存不受影响。")}</p><label>{t("输入名称确认")}<input autoFocus value={name} onChange={e=>setName(e.target.value)} placeholder={item.instance.name}/></label><button className="button danger full" disabled={pending||item.running||name!==item.instance.name} onClick={()=>act('instance-delete',{id:item.instance.instanceId,name,permanent:true})}>{pending?t("正在永久删除…"):t("永久删除，无法恢复")}</button></>:<><h2>{t("回收站")}</h2><p>{t("这里保留已删除实例的全部本地数据。恢复不会自动开服或恢复邀请及服务器绑定。")}</p>{entries.length===0?<div className="empty">{t("回收站为空")}</div>:<div className="version-list">{entries.map(e=><button key={e.id} disabled={pending} onClick={()=>act('instance-restore',{id:e.id})}><span><strong>{e.name}</strong><small>{e.server?t("服务器"):t("游戏实例")} · Minecraft {e.minecraft}</small></span><span>{t("恢复")}</span></button>)}</div>}{pending&&<p>{t("正在恢复…")}</p>}</>;
}
