import {t,getLocale,ServiceMessage} from './i18n';
import React,{useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
const bytes=(n:number)=>n>=1073741824?`${(n/1073741824).toFixed(1)} GB`:`${(n/1048576).toFixed(1)} MB`;
export function JobProgress({job,onRefresh,hideRetry=false}:{job:Json;onRefresh:()=>void;hideRetry?:boolean}){
 const [error,setError]=useState(''),[pending,setPending]=useState(false);const active=['running','queued'].includes(job.status),d=job.download;
 const action=async(action:string)=>{setPending(true);setError('');try{await invoke('call',{action,payload:{jobId:job.id}});onRefresh()}catch(e){setError(String(e))}finally{setPending(false)}};
 return <div className="task-progress" aria-live="polite"><div><strong>{job.cancelRequested&&active?t("正在安全停止…"):<ServiceMessage text={job.message} progress={active}/>}</strong><span>{job.status==='done'?t("已完成"):job.status==='error'?t("失败"):job.status==='cancelled'?t("已取消"):job.status==='queued'?t("排队中"):t("进行中")}</span></div>{active&&d&&<><progress aria-label={t("当前文件下载进度")} max={d.total||1} value={d.total?Math.min(d.received,d.total):undefined}/><small>{bytes(d.received)}{d.total?` / ${bytes(d.total)}`:''}{d.speed>0?` · ${bytes(d.speed)}/s`:''} {t(" · 当前文件")}</small></>}{active&&!d&&<div className="indeterminate"/>}<div className="actions">{active&&job.cancelable&&<button className="button" disabled={pending||job.cancelRequested} onClick={()=>action('job-cancel')}>{job.cancelRequested?t("取消中…"):t("取消任务")}</button>}{['error','cancelled'].includes(job.status)&&job.retryable&&!hideRetry&&<button className="button" disabled={pending} onClick={()=>action('job-retry')}>{t("重试任务")}</button>}</div>{error&&<p role="alert">{<ServiceMessage text={error}/>}</p>}</div>
}
