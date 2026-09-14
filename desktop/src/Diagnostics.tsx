import {t,getLocale,ServiceMessage} from './i18n';
import React,{useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
const rules=[
 {pattern:/OutOfMemoryError|Could not reserve enough space/i,title:'内存不足',tip:'关闭其他程序；在实例设置中调整内存。分配过多也可能导致 Java 无法启动。'},
 {pattern:/UnsupportedClassVersionError|class file version/i,title:'Java 版本不匹配',tip:'在设置中清空自定义 Java 路径，再检查并修复安装，使用此游戏版本的托管 Java。'},
 {pattern:/Incompatible mods|requires .* version|Mod resolution encountered|Missing.*dependenc/i,title:'Mod 依赖或版本冲突',tip:'根据下面的日志补齐依赖；若刚更新过，可在更新与恢复中还原 Mod 快照。'},
 {pattern:/Address already in use|FAILED TO BIND TO PORT/i,title:'服务器端口被占用',tip:'停止占用端口的服务器，或在实例设置中更换端口。'},
 {pattern:/session.lock|already locked|world.*in use/i,title:'存档正在被使用',tip:'关闭使用同一存档的游戏或服务器后重试。不要删除正在使用的锁文件。'},
 {pattern:/Invalid session|Failed to verify username|Authentication servers/i,title:'身份验证失败',tip:'确认服务器的验证模式与玩家档案匹配；正版服务器需要有效的正版登录。'}
];
export function Diagnostics({id}:{id:string}){
 const [result,setResult]=useState<{title:string;tip:string;evidence:string}[]|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false);
 const check=async()=>{setBusy(true);setError('');try{const r=await invoke<{text:string}>('call',{action:'logs',payload:{id}});setResult(rules.flatMap(rule=>{const line=r.text.split('\n').find(l=>rule.pattern.test(l));return line?[{...rule,evidence:line.slice(0,600)}]:[]}))}catch(e){setError(String(e))}finally{setBusy(false)}};
 return <section className="panel settings-card"><h2>{t("启动诊断")}</h2><p>{t("在本机分析最近运行日志，给出可能原因。安装下载失败请同时查看任务中的错误。")}</p><button className="button" disabled={busy} onClick={check}>{busy?t("分析中…"):t("分析最近运行日志")}</button>{error&&<div className="alert">{<ServiceMessage text={error}/>}</div>}{result?.length===0&&<p>{t("日志中没有匹配到已知原因，或尚无日志。请在运行日志和任务中查看具体错误。")}</p>}{result?.map(r=><div key={r.title}><h3>{<ServiceMessage text={r.title}/>}</h3><p>{<ServiceMessage text={r.tip}/>}</p><pre style={{whiteSpace:'pre-wrap',overflowWrap:'anywhere'}}>{r.evidence}</pre></div>)}</section>
}
