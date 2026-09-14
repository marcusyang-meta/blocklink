import {t,getLocale,ServiceMessage} from './i18n';
import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
export function WorldBackups({item,jobs}:{item:Json;jobs:Json[]}){
 const [items,setItems]=useState<Json[]>([]),[error,setError]=useState(''),[pending,setPending]=useState(false);
 const id=item.instance.instanceId;
 useEffect(()=>{invoke<Json>('call',{action:'world-backups',payload:{id}}).then(r=>setItems(r.items)).catch(e=>setError(String(e)))},[id,jobs.filter(j=>j.status==='done').length]);
 const locked=pending||item.running||jobs.some(j=>['queued','running'].includes(j.status));
 const run=async(action:string,extra:Json={})=>{setPending(true);setError('');try{await invoke('call',{action,payload:{id,...extra}})}catch(e){setError(String(e))}finally{setPending(false)}};
 return <section className="panel settings-card"><div className="section-heading"><h2>{t("世界备份")}</h2><button className="button" disabled={locked} onClick={()=>run('world-backup')}>{t("立即备份全部世界")}</button></div><p>{t("应用 Mod 更新或回滚前自动备份。备份保存在本机，暂不自动清理，请留意磁盘空间。恢复会新增一个世界，保留当前进度；服务器需在世界列表手动切换。")}</p>{error&&<div className="alert">{<ServiceMessage text={error}/>}</div>}{!items.length&&<p>{t("还没有备份。")}</p>}{items.map(b=><div className="job-row" key={b.id}><div><strong>{b.name} · {<ServiceMessage text={b.reason}/>}</strong><p>{new Date(b.createdAt*1000).toLocaleString(getLocale())} · {b.minecraft}</p></div><button className="button" disabled={locked} onClick={()=>run('world-backup-restore',{backupId:b.id})}>{t("恢复为新世界")}</button></div>)}</section>
}
