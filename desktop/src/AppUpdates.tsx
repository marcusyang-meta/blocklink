import React,{useEffect,useState} from 'react';
import {invoke,Channel} from '@tauri-apps/api/core';
import {RefreshCw} from 'lucide-react';
import {t,ServiceMessage} from './i18n';
export function AppUpdates(){
 const [update,setUpdate]=useState<any>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState(''),[download,setDownload]=useState<any>(null);
 const check=async(quiet=false)=>{setBusy(true);if(!quiet)setMessage('');try{const found=await invoke('check_update');setUpdate(found);if(!quiet&&!found)setMessage(t('已经是最新版本'))}catch(e){if(!quiet)setMessage(String(e))}finally{setBusy(false)}};
 useEffect(()=>{void check(true)},[]);
 const install=async()=>{setBusy(true);setMessage('');try{const progress=new Channel<any>();progress.onmessage=setDownload;await invoke('install_update',{progress});setMessage(t('更新完成，正在重新打开…'))}catch(e){setMessage(String(e));setDownload(null);setUpdate(null)}finally{setBusy(false)}};
 return <div className="app-updates"><button className="nav" disabled={busy} onClick={()=>check()}><RefreshCw size={18} className={busy?'spin':''}/>{t('检查启动器更新')}</button>{update&&<div className="update-notice"><strong>{t('新版本 {0}',update.version)}</strong><p>{t('完成下载后将重新打开启动器，请先结束游戏和开服。')}</p><button className="button" disabled={busy} onClick={install}>{t('下载并更新')}</button></div>}{download&&<progress aria-label={t('下载更新')} value={download.received} max={download.total||undefined}/>} {message&&<small role="status"><ServiceMessage text={message}/></small>}</div>
}
