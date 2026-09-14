import {t,getLocale,ServiceMessage} from './i18n';
import React,{useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json=Record<string,any>;
export function Recovery({item,jobs,onStart,onSettings,onRestore,onLogs,onRefresh}:{item:Json;jobs:Json[];onStart:()=>void;onSettings:()=>void;onRestore:()=>void;onLogs:()=>void;onRefresh:()=>void}){
 const [dismissed,setDismissed]=useState(''),[text,setText]=useState(''),[error,setError]=useState(''),[pending,setPending]=useState(false);
 const id=item.instance.instanceId;const latest=jobs.filter(j=>j.instanceId===id&&['launch',t("运行进程"),'install'].includes(j.action)).slice(-1)[0];
 const failed=latest?.status==='error'&&!item.running&&latest.id!==dismissed;
 useEffect(()=>{setText('');if(failed)invoke<Json>('call',{action:'logs',payload:{id}}).then(r=>setText(r.text)).catch(()=>{})},[id,latest?.id,failed]);
 if(!failed)return null;
 const combined=latest.message+'\n'+text;const memory=/OutOfMemoryError|Could not reserve enough space/i.test(combined),java=/UnsupportedClassVersionError|class file version/i.test(combined),mods=/Incompatible mods|requires .* version|Mod resolution encountered|Missing.*dependenc/i.test(combined);
 const locked=pending||jobs.some(j=>j.instanceId===id&&['queued','running'].includes(j.status));
 const repair=async()=>{setPending(true);setError('');try{await invoke('call',{action:'install',payload:{id}});onRefresh()}catch(e){setError(String(e))}finally{setPending(false)}};
 return <section className="panel settings-card recovery-card" role="alert"><h2>{memory?t("游戏需要调整内存"):java?t("游戏使用的 Java 可能不匹配"):mods?t("有些模组没能一起运行"):t("这次没能顺利进入游戏")}</h2><p>{memory?t("可以调整分配的内存，再试一次。"):java?t("如果指定了自定义 Java，请先在设置中清空路径，再修复运行环境。"):mods?t("如果刚改过模组，可以选择恢复修改前的环境，再启动游戏。"):t("可以重试；下载或游戏文件有问题时，修复会重新校验并补齐所需文件。")}</p><div className="actions"><button className="button primary" disabled={locked} onClick={onStart}>{t("重新启动")}</button><button className="button" disabled={locked} onClick={repair}>{t("修复游戏文件")}</button>{(memory||java)&&<button className="button" onClick={onSettings}>{t("调整运行设置")}</button>}<button className="button" onClick={onRestore}>{t("选择历史环境恢复")}</button><button className="button" onClick={onLogs}>{t("查看运行日志")}</button><button className="text-link" onClick={()=>setDismissed(latest.id)}>{t("暂时收起")}</button></div><details><summary>{t("错误详情")}</summary><p>{latest.message}</p></details>{error&&<p>{<ServiceMessage text={error}/>}</p>}</section>
}
