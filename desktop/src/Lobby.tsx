import {t,getLocale,ServiceMessage} from './i18n';
import React, {useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
type Json = Record<string, any>;
const api = (action:string,payload:Json={}) => invoke<any>('call',{action,payload});
export function LobbySettings({url,onSaved}:{url:string,onSaved:()=>void}) {
  const [address,setAddress]=useState(url),[key,setKey]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false);
  return <section><h3>{t("Cloudflare 联机大厅")}</h3><p>{t("房主配置大厅地址和开房凭据。朋友只需粘贴邀请链接。")}</p><label>{t("大厅地址")}<input value={address} placeholder={t("https://blocklink-lobby.你的子域.workers.dev")} onChange={e=>setAddress(e.target.value)}/></label><label>{t("开房凭据")}<input type="password" autoComplete="off" value={key} placeholder={t("已保存时可留空")} onChange={e=>setKey(e.target.value)}/></label><button className="button" disabled={busy||!address} onClick={async()=>{setBusy(true);try{await api('lobby-configure',{url:address,hostKey:key});setKey('');setMessage(t("大厅设置已保存"));onSaved()}catch(e){setMessage(String(e))}finally{setBusy(false)}}}>{t("保存大厅设置")}</button><p role="status">{message}</p></section>;
}
export function LobbyRoom({id,session,configured,onRefresh}:{id:string,session?:Json,configured:boolean,onRefresh:()=>void}) {
  const [invitation,setInvitation]=useState(''),[busy,setBusy]=useState(false),[message,setMessage]=useState('');
  const room=session?.room; const shareLink=invitation||session?.invitation;
  const create=async()=>{setBusy(true);setMessage(t("正在创建房间…"));try{
    const queued=await api('lobby-create',{id});
    const deadline=Date.now()+90000;
    while(Date.now()<deadline){const state=await api('status');const job=state.jobs.find((j:Json)=>j.id===queued.jobId);if(job?.status==='error')throw Error(job.message);if(job?.status==='done'){setInvitation(job.result.invitation);setMessage(t("房间已开启，请将邀请发给朋友"));onRefresh();return}await new Promise(resolve=>setTimeout(resolve,500));}
    throw Error(t("开房仍在处理中，请查看任务与下载"));
  }catch(e){setMessage(String(e))}finally{setBusy(false)}};
  return <><h2>{t("邀请朋友一起玩")}</h2><p>{t("Cloudflare 大厅负责找到房主。游戏与 Mods 优先直连，无法直连时通过 Cloudflare TURN 加密中继。")}</p>
    {!configured&&<div className="hint"><p>{t("请先在「启动器设置」中配置你部署的 Cloudflare 大厅。")}</p></div>}
    {session&&<div className="panel settings-card"><h3>{room?.name||t("联机房间")}</h3><p>{session.connected?t("大厅已连接"):t("大厅已断开")} · {room?.running?t("游戏服务器运行中"):t("等待房主启动游戏服务器")}</p><p>{room?.minecraft} · {room?.modCount??0} {t(" 个 Mod · ")}{room?.members?.length??0} {t(" 位朋友在线")}</p>{room?.members?.map((m:Json)=><span className="tag" key={m.id}>{m.name}</span>)}{room?.expiresAt&&<small>{t("邀请有效至 ")}{new Date(room.expiresAt).toLocaleString(getLocale())}</small>}{room?.error&&<p role="alert">{room.error}</p>}</div>}
    <button className="button primary full" disabled={busy||!configured||session?.connected} onClick={create}>{busy?t("正在开房…"):t("开启房间并生成邀请")}</button>
    {session&&<button className="button full" disabled={busy} onClick={async()=>{try{await api('lobby-close',{id});setInvitation('');setMessage(t("房间已关闭，旧邀请已失效"));onRefresh()}catch(e){setMessage(String(e))}}}>{t("关闭房间并断开联机")}</button>}
    {shareLink&&<label>{t("邀请链接")}<textarea readOnly value={shareLink} onFocus={e=>e.target.select()}/><button className="button" onClick={async()=>{try{await navigator.clipboard.writeText(shareLink);setMessage(t("邀请已复制"))}catch{setMessage(t("请选中上方链接并复制"))}}}>{t("复制邀请")}</button></label>}
    <p role="status">{message}</p><p>{t("开房不会自动接受游戏协议或启动服务器。准备好后点击「启动服务器」。关闭窗口后后台继续运行；退出后台或关闭电脑会使房主离线。")}</p></>;
}
