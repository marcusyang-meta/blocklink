import React,{useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
import {t,ServiceMessage} from './i18n';
export function PackExport({id,busy,onRefresh}:{id:string;busy:boolean;onRefresh:()=>void}){
 const [pending,setPending]=useState(false),[message,setMessage]=useState('');
 const exportPack=async()=>{setPending(true);setMessage('');try{const path=await invoke<string|null>('save_pack');if(path){await invoke('call',{action:'pack-export',payload:{id,path}});setMessage(t('已添加导出任务，可在任务与下载查看结果。'));onRefresh()}}catch(e){setMessage(String(e))}finally{setPending(false)}};
 return <section className="panel settings-card"><h2>{t('导出玩法')}</h2><p>{t('包含模组、配置、脚本、资源包和光影，不包含存档和账户。分享前请检查配置中的私人信息，并确认内容允许再分发。')}</p><button className="button" disabled={busy||pending} onClick={exportPack}>{t('导出为 .mrpack')}</button>{message&&<p role="status"><ServiceMessage text={message}/></p>}</section>
}
