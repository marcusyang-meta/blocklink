use super::*;
use std::io::{Cursor,Read};
fn metadata<R:Read+std::io::Seek>(reader:R, depth:usize, result:&mut Vec<Value>)->Result<()> {
    anyhow::ensure!(depth<8 && result.len()<4096,"嵌套 Mod 过多");
    let mut zip=zip::ZipArchive::new(reader)?;
    if !zip.file_names().any(|n|n=="fabric.mod.json") {return Ok(());}
    let mut text=String::new();zip.by_name("fabric.mod.json")?.take(1_048_577).read_to_string(&mut text)?;
    anyhow::ensure!(text.len()<=1_048_576,"Mod 描述过大");
    let meta=mods::parse_mod_json(&text)?;
    let nested:Vec<String>=meta["jars"].as_array().into_iter().flatten().filter_map(|v|v["file"].as_str().map(str::to_owned)).collect();
    result.push(meta);
    for name in nested {
        let mut bytes=Vec::new();zip.by_name(&name)?.take(64*1024*1024+1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len()<=64*1024*1024,"内嵌 Mod 过大");
        metadata(Cursor::new(bytes),depth+1,result)?;
    }
    Ok(())
}
fn normalize(v:&str)->String {
    let parts:Vec<_>=v.splitn(2,'+').collect();
    let (core,pre)=parts[0].split_once('-').map_or((parts[0],None),|(c,p)|(c,Some(p)));
    let mut base=core.to_owned();
    while base.matches('.').count()<2 {base.push_str(".0");}
    if let Some(pre)=pre {base.push('-');base.push_str(pre);}
    if parts.len()==2 {base.push('+');base.push_str(parts[1]);}
    base
}
fn matches(req:&Value,actual:&str)->Result<bool> {
    if let Some(a)=req.as_array() {for v in a {if matches(v,actual)? {return Ok(true);}}return Ok(false);}
    let r=req.as_str().context("未知版本表达式")?;
    // Fabric comparisons include prereleases in ordinary ordered ranges.
    // VersionReq's npm-style prerelease exclusion would reject 0.5.0-beta
    // against >=0.3.2, despite it being newer under Fabric's comparison.
    for token in r.split_whitespace() {
        let ordered=[">=","<=",">","<","=","~","^"].into_iter().find(|op|token.starts_with(op));
        let bare=token.as_bytes().first().is_some_and(u8::is_ascii_digit) && !token.contains(['*','x','X']);
        if ordered.is_some() || bare {
            let op=ordered.unwrap_or("=");
            let value=if ordered.is_some(){&token[op.len()..]}else{token};
            let value=if value.ends_with('-'){format!("{value}0")}else{value.to_owned()};
            let actual=semver::Version::parse(&normalize(actual))?;
            let minimum=semver::Version::parse(&normalize(&value))?;
            let comparison=actual.cmp_precedence(&minimum);
            let ok=match op {">="=>!comparison.is_lt(),"<="=>!comparison.is_gt(),">"=>comparison.is_gt(),"<"=>comparison.is_lt(),"~"=>!comparison.is_lt() && actual.major==minimum.major && actual.minor==minimum.minor,"^"=>!comparison.is_lt() && actual.major==minimum.major,_=>comparison.is_eq()};
            if !ok{return Ok(false);}
        }else {
            let token=token.strip_suffix('-').unwrap_or(token);
            if !mods::loader_matches(&json!(token),&normalize(actual))? {return Ok(false);}
        }
    }
    anyhow::ensure!(!r.trim().is_empty(),"未知版本表达式");
    Ok(true)
}
pub(super) fn inspect(ws:&Workspace,i:&Instance,java:Option<u64>)->Result<Value> {inspect_impl(ws,i,java,None)}
pub(super) fn inspect_target(ws:&Workspace,i:&Instance,target:&Lockfile)->Result<Value> {inspect_impl(ws,i,None,Some(target))}
fn inspect_impl(ws:&Workspace,i:&Instance,java:Option<u64>,target:Option<&Lockfile>)->Result<Value> {
    let mut errors=Vec::new();let mut warnings=Vec::new();
    if target.is_none() {if let Err(e)=ws.verify_instance(&i.instance_id) {if ws.read_lock(&i.instance_id)?.is_some(){errors.push(format!("Mod 文件校验失败：{e:#}"));}}}
    let dir=ws.instance_dir(&i.instance_id)?.join("game/mods");
    let mut metas=Vec::new();let mut top=HashMap::new();
    let paths:Vec<PathBuf>=if let Some(target)=target {target.mods.iter().map(|m|ws.blob_path(&m.sha512).map_err(anyhow::Error::from)).collect::<Result<_>>()?}else if dir.exists(){fs::read_dir(&dir)?.map(|e|Ok(e?.path())).collect::<Result<_>>()?}else{vec![]};
    if matches!(i.loader,Loader::Fabric{..}) {
        for path in paths {
            if target.is_none() && path.extension().and_then(|s|s.to_str())!=Some("jar"){continue;}
            let start=metas.len();
            match metadata(fs::File::open(&path)?,0,&mut metas) {
                Ok(())=>{if let Some(m)=metas.get(start){let id=field(m,"id")?;if top.insert(id.to_owned(),path.clone()).is_some(){errors.push(format!("重复 Mod：{id}"));}}else{errors.push(format!("{} 缺少 Fabric 描述",path.file_name().unwrap().to_string_lossy()));}},
                Err(e)=>errors.push(format!("{}：{e:#}",path.file_name().unwrap().to_string_lossy()))
            }
        }
        let mut installed:HashMap<String,String>=HashMap::new();
        installed.insert("minecraft".into(),i.minecraft.clone());
        if let Loader::Fabric{version}=&i.loader {installed.insert("fabricloader".into(),version.clone());}
        if let Some(java)=java {installed.insert("java".into(),java.to_string());}
        for m in &metas {
            let id=field(m,"id")?.to_owned();let version=field(m,"version")?.to_owned();
            let replace=installed.get(&id).is_none_or(|old|semver::Version::parse(&normalize(&version)).ok()>semver::Version::parse(&normalize(old)).ok());
            if replace {installed.insert(id,version.clone());}
            for alias in m["provides"].as_array().into_iter().flatten().filter_map(Value::as_str){installed.entry(alias.into()).or_insert(version.clone());}
        }
        for m in &metas {
            let id=field(m,"id")?;
            for (kind,required) in [("depends",true),("breaks",false)] {
                for (dep,req) in m[kind].as_object().into_iter().flatten() {
                    if dep=="java" && java.is_none(){continue;}
                    if let Some(actual)=installed.get(dep) {
                        match matches(req,actual) {
                            Ok(ok) if ok!=required=>errors.push(format!("{id} {} {dep} {req}，当前为 {actual}",if required {"需要"}else{"不兼容"})),
                            Err(_)=>warnings.push(format!("{id} 对 {dep} 的要求 {req} 无法完整判断")),
                            _=>{}
                        }
                    }else if required {errors.push(format!("{id} 缺少依赖 {dep} {req}"));}
                }
            }
        }
    }else if !matches!(i.loader,Loader::Vanilla){warnings.push("此 Loader 暂仅检查共享文件完整性，未解析全部运行依赖".into());}
    if target.is_none(){match crate::shaders::warnings(ws,i){Ok(w)=>warnings.extend(w),Err(e)=>warnings.push(format!("光影检查失败：{e:#}"))}}
    errors.sort();errors.dedup();warnings.sort();warnings.dedup();
    Ok(json!({"ready":errors.is_empty(),"errors":errors,"warnings":warnings,"checked":metas.len()}))
}
pub(super) fn require_ready(ws:&Workspace,i:&Instance,java:Option<u64>)->Result<()> {
    let result=inspect(ws,i,java)?;
    anyhow::ensure!(result["ready"]==true,"启动前检查未通过：{}。请打开启动检查或兼容适配。",result["errors"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join("；"));
    Ok(())
}

#[cfg(test)] mod tests {
 use super::*;
 use std::io::Write;
 fn instance(ws:&Workspace)->Result<Instance>{let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":uuid::Uuid::new_v4().to_string(),"name":"QA","minecraft":"1.21.4","loader":{"kind":"fabric","version":"0.16.10"},"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}))?;ws.create_instance(&i)?;Ok(i)}
 #[test] fn detects_local_dependencies_loader_java_and_duplicates()->Result<()> {
  let temp=tempfile::tempdir()?;let ws=Workspace::open(temp.path())?;let i=instance(&ws)?;
  let dir=ws.instance_dir(&i.instance_id)?.join("game/mods");fs::create_dir_all(&dir)?;
  let path=dir.join("qa.jar");let mut zip=zip::ZipWriter::new(fs::File::create(&path)?);
  zip.start_file("fabric.mod.json",zip::write::SimpleFileOptions::default())?;
  zip.write_all(serde_json::to_string(&json!({"id":"example","version":"1.0.0","depends":{"minecraft":"1.21.4","fabricloader":">=0.18.0","java":">=21","missing":"*"}}))?.as_bytes())?;zip.finish()?;
  let result=inspect(&ws,&i,Some(17))?;let errors=result["errors"].to_string();
  assert!(errors.contains("fabricloader"));assert!(errors.contains("java"));assert!(errors.contains("missing"));assert!(!errors.contains("需要 minecraft"));
  fs::copy(&path,dir.join("duplicate.jar"))?;assert!(inspect(&ws,&i,None)?["errors"].to_string().contains("重复 Mod"));
  assert!(matches(&json!("~26.2-"),"26.2")?);assert!(!matches(&json!("1.21.4"),"1.21.5")?);Ok(())
 }
 #[test] fn fabric_ordered_ranges_include_newer_prereleases()->Result<()> {
  assert!(matches(&json!(">=0.3.2"),"0.5.0-beta.4")?);
  assert!(!matches(&json!(">=0.5.0"),"0.5.0-beta.4")?);
  assert!(matches(&json!(">=1.21.4- <1.21.5-"),"1.21.4")?);
  assert!(!matches(&json!(">=1.21.4- <1.21.5-"),"1.21.5")?);
  assert!(matches(&json!(">=1.15-alpha.19.38.b"),"1.21.4")?);
  Ok(())
 }
 #[test] fn environment_transition_and_interrupted_recovery()->Result<()> {
  let temp=tempfile::tempdir()?;let ws=Workspace::open(temp.path())?;let i=instance(&ws)?;let old=mods::lock(&ws,&i)?;mods::apply(&ws,&i,&old)?;
  let mut target=i.clone();target.loader=Loader::Fabric{version:"0.18.0".into()};let mut next=old.clone();next.environment.loader=target.loader.clone();
  mods::transition(&ws,&i,&target,&next)?;assert_eq!(ws.instance(&i.instance_id)?.loader,target.loader);
  mods::transition(&ws,&target,&i,&old)?;assert_eq!(ws.read_lock(&i.instance_id)?.unwrap().environment.loader,i.loader);
  let dir=ws.instance_dir(&i.instance_id)?;
  write_json(&dir.join("environment-transition.json"),&json!({"instance":i,"lock":old}))?;
  write_json(&dir.join("instance.json"),&target)?;
  mods::recover_environments(&ws)?;assert_eq!(ws.instance(&i.instance_id)?.loader,i.loader);assert!(!dir.join("environment-transition.json").exists());Ok(())
 }
}
