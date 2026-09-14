import React, {useState,useSyncExternalStore} from 'react';
import english from './locales/en.json';

export type Locale = 'zh-CN' | 'en';
const key='blocklink.language';
const catalog:Record<string,string>=english;
const reverse=new Map(Object.entries(catalog).map(([zh,en])=>[en,zh]));
export function resolveLocale(saved:string|null, languages:readonly string[]):Locale {
 if(saved==='zh-CN'||saved==='en')return saved;
 return languages[0]?.toLowerCase().startsWith('zh')?'zh-CN':'en';
}
let locale:Locale=resolveLocale(null,typeof navigator==='undefined'?['zh-CN']:navigator.languages);
try {locale=resolveLocale(localStorage.getItem(key),navigator.languages)} catch {/* Storage may be unavailable. */}
const listeners=new Set<()=>void>();
function subscribe(fn:()=>void){listeners.add(fn);return()=>{listeners.delete(fn)}}
export const getLocale=()=>locale;
export function setLocale(value:Locale){
 if(value!=='zh-CN'&&value!=='en')return;
 locale=value;
 try{localStorage.setItem(key,value)}catch{/* Language still changes for this session. */}
 if(typeof document!=='undefined')document.documentElement.lang=value;
 listeners.forEach(fn=>fn());
}
if(typeof document!=='undefined')document.documentElement.lang=locale;
export function useLocale(){return useSyncExternalStore(subscribe,getLocale,getLocale)}
export function translate(source:string, language:Locale, values:readonly unknown[]=[]):string {
 const trimmed=source.trim();
 const canonical=catalog[trimmed]!==undefined?trimmed:reverse.get(trimmed)||trimmed;
 const core=language==='en'?(catalog[canonical]??trimmed):canonical;
 const text=source.slice(0,source.length-source.trimStart().length)+core+source.slice(source.trimEnd().length);
 return text.replace(/\{(\d+)\}/g,(match,index)=>Number(index)<values.length?String(values[Number(index)]):match);
}
export const t=(source:string,...values:unknown[])=>translate(source,locale,values);

const serviceText:Record<string,string>={
 '正在检查游戏环境':'Checking the game setup',
 '正在自动匹配游戏内容':'Matching compatible game content',
 '正在备份存档':'Backing up saves',
 '正在准备兼容的游戏内容':'Preparing compatible game content',
 '正在准备适合这个游戏的光影':'Preparing compatible shaders',
 '正在匹配光影效果':'Matching shader effects',
 '识别本地 Mod 文件来源':'Identifying local mod sources',
 '检查依赖组合，冲突时尝试较旧候选版本':'Checking dependencies and alternative versions',
 '检查服务器已发布的 Mod 更新':'Checking published server mod updates',
 '服务器 Mod 已同步':'Server mods synced',
 '建立游戏联机通道':'Connecting to the game server',
 '安装服务器与 Loader':'Installing server and loader',
 '安装 Minecraft 客户端':'Installing the Minecraft client',
};
export function translateService(raw:string,language:Locale):string {
 if(language==='zh-CN')return raw;
 if(serviceText[raw])return serviceText[raw];
 const exact=translate(raw,language);
 if(exact!==raw)return exact;
 for(const [prefix,en] of [['自动下载 Java ','Downloading Java '],['安装游戏依赖 · ','Installing game library · '],['安装资源 · ','Installing assets · '],['检查更新 · ','Checking updates · '],['匹配 ','Matching ']]){
  if(raw.startsWith(prefix))return en+raw.slice(prefix.length);
 }
 return raw;
}

// Logs and third-party errors are preserved verbatim; unknown service messages
// get an English summary with the original available for troubleshooting.
export function ServiceMessage({text,progress=false}:{text:unknown;progress?:boolean}){
 useLocale();const [expanded,setExpanded]=useState(false);const raw=String(text??'');
 const translated=translateService(raw,locale);
 if(locale==='zh-CN'||!/[\u3400-\u9fff]/.test(translated))return <>{translated}</>;
 return <span className="service-message">{progress?'Working on your request…':'This operation needs attention.'} <button type="button" className="text-link" aria-expanded={expanded} onClick={()=>setExpanded(!expanded)}>Original details</button>{expanded&&<span className="original-message">{raw}</span>}</span>;
}
export function LanguagePicker(){
 const language=useLocale();
 return <label className="language-setting"><span>{language==='en'?'Language':'语言'}<small>{language==='en'?'Applies immediately. Your games stay unchanged.':'立即生效，不影响游戏与存档。'}</small></span><select aria-label="Language / 语言" value={language} onChange={e=>setLocale(e.target.value as Locale)}><option value="zh-CN">简体中文</option><option value="en">English</option></select></label>;
}
