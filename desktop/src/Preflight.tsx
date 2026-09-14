import {t,getLocale,ServiceMessage} from './i18n';
import React,{useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
export function Preflight({id,onRepair}:{id:string;onRepair:()=>void}){
 const [result,setResult]=useState<Json|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 const check=async()=>{setBusy(true);setError('');try{setResult(await invoke<Json>('call',{action:'preflight',payload:{id}}))}catch(e){setError(String(e))}finally{setBusy(false)}};
 return <section className="panel settings-card"><h2>{t("启动前检查")}</h2><p>{t("启动时自动检查；这里可以提前查看重复 Mod、版本要求、依赖与文件损坏。Fabric 支持读取内嵌依赖；其他 Loader 暂以文件校验为主。Java 要求在启动时结合实际运行环境检查。")}</p><div className="actions"><button className="button" disabled={busy} onClick={check}>{busy?t("检查中…"):t("检查当前环境")}</button><button className="button" onClick={onRepair}>{t("打开兼容适配")}</button></div>{error&&<div className="alert">{<ServiceMessage text={error}/>}</div>}{result&&<><h3>{result.ready?t("未发现阻塞问题"):t("发现启动问题")}</h3>{result.errors.map((e:string)=><p className="alert" key={e}>{<ServiceMessage text={e}/>}</p>)}{result.warnings.map((e:string)=><p className="hint" key={e}>{<ServiceMessage text={e}/>}</p>)}</>}</section>
}
