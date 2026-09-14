import {t,getLocale,ServiceMessage} from './i18n';
import {useEffect,useState} from 'react';
import {invoke} from '@tauri-apps/api/core';
export type Memory={id:string;lastLaunchedAt:number|null;world:{name:string;lastPlayed:number}|null;cover:string|null;artKey:string|null};
const call=<T,>(action:string,payload:Record<string,unknown>={})=>invoke<T>('call',{action,payload});
export function useHomeMemory(){
 const [items,setItems]=useState<Memory[]>([]);
 useEffect(()=>{let active=true,pending=false;const update=async()=>{if(pending)return;pending=true;try{const r=await call<{items:Memory[]}>('home-summary');if(active)setItems(r.items)}catch{/* The launcher remains usable without optional home metadata. */}finally{pending=false}};void update();const timer=setInterval(update,15000);return()=>{active=false;clearInterval(timer)}},[]);
 return items;
}
export function useHomeArt(id:string|undefined,key:string|null|undefined){
 const [art,setArt]=useState<{id:string;key:string;image:string}|null>(null);
 useEffect(()=>{let active=true;let url:string|undefined;if(!id||!key){setArt(null);return}void call<{image:string|null}>('home-art',{id}).then(r=>{if(!r.image||!active){if(active)setArt(null);return}if(!r.image.startsWith('data:image/png;base64,'))throw Error('Invalid artwork');const binary=atob(r.image.slice(22));const bytes=Uint8Array.from(binary,c=>c.charCodeAt(0));url=URL.createObjectURL(new Blob([bytes],{type:'image/png'}));const img=new Image();img.onload=()=>{if(active)setArt({id,key,image:url!})};img.onerror=()=>{if(active)setArt(null)};img.src=url}).catch(()=>{if(active)setArt(null)});return()=>{active=false;if(url)URL.revokeObjectURL(url)}},[id,key]);
 return art && art.id===id && art.key===key?art.image:null;
}
export const playedTime=(timestamp:number)=>new Date(timestamp).toLocaleString(getLocale(),{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'});
export const recentTime=(m:Memory)=>Math.max(m.lastLaunchedAt||0,m.world?.lastPlayed||0);

