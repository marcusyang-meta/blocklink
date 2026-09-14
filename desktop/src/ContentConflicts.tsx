import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t,ServiceMessage} from './i18n';
type Json=Record<string,any>;
export function ContentConflicts({item,jobs,onRefresh}:{item:Json;jobs:Json[];onRefresh:()=>void}){
 const id=item.instance.instanceId;
 const [files,setFiles]=useState<string[]>([]),[dismissed,setDismissed]=useState(false),[error,setError]=useState(''),[pending,setPending]=useState(false);
 const revision=jobs.filter(j=>j.instanceId===id).map(j=>j.id+':'+j.status).join('|');
 useEffect(()=>{let active=true;invoke<Json>('call',{action:'content-conflicts',payload:{id}}).then(v=>{if(active){setFiles(v.files);setDismissed(false)}}).catch(()=>{});return()=>{active=false}},[id,revision]);
 const busy=pending||item.running||jobs.some(j=>j.instanceId===id&&['queued','running'].includes(j.status));
 if(!files.length||dismissed)return null;
 const resolve=async()=>{setPending(true);setError('');try{await invoke('call',{action:'content-resolve',payload:{id}});onRefresh()}catch(e){setError(String(e))}finally{setPending(false)}};
 return <section className="panel settings-card recovery-card" role="alert"><h2>{t('你的设置与服务器不同')}</h2><p>{t('使用服务器版本前，会先备份你的修改。保留本地设置会暂停同步，暂时无法加入这个房间。')}</p><details><summary>{t('查看受影响的文件（{0}）',files.length)}</summary><ul>{files.map(f=><li key={f}>{f}</li>)}</ul></details><div className="actions"><button className="button primary" disabled={busy} onClick={resolve}>{t('备份并使用服务器版本')}</button><button className="button" disabled={pending} onClick={()=>setDismissed(true)}>{t('保留本地，暂不同步')}</button><button className="text-link" onClick={()=>invoke('open_content_backups',{id}).catch(e=>setError(String(e)))}>{t('查看设置备份')}</button></div>{error&&<p><ServiceMessage text={error}/></p>}</section>
}
