use super::*;

fn dirs(ws:&Workspace,i:&Instance)->Result<PathBuf>{
    let dir=ws.instance_dir(&i.instance_id)?;
    for rel in ["game","game/config","game/shaderpacks"] {let p=dir.join(rel);if p.exists(){anyhow::ensure!(!fs::symlink_metadata(&p)?.file_type().is_symlink(),"光影目录不能是符号链接");}}
    Ok(dir)
}
fn regular(path:&Path)->Result<()> {anyhow::ensure!(fs::symlink_metadata(path)?.file_type().is_file(),"光影文件必须是普通文件");Ok(())}
fn config(dir:&Path)->Result<String>{let p=dir.join("game/config/iris.properties");if !p.exists(){return Ok(String::new());}regular(&p)?;Ok(fs::read_to_string(p)?)}
pub fn capture(ws:&Workspace,i:&Instance)->Result<String>{config(&dirs(ws,i)?)}
pub fn restore_config(ws:&Workspace,i:&Instance,text:&str)->Result<()>{write_config(&dirs(ws,i)?,text)}
fn prop(text:&str,key:&str)->String {text.lines().filter_map(|l|l.trim().split_once('=')).filter(|(k,_)|k.trim()==key).last().map(|(_,v)|v.trim().replace("\\ "," ").replace("\\:",":").replace("\\=","=").replace("\\\\","\\")).unwrap_or_default()}
fn write_config(dir:&Path,text:&str)->Result<()> {
    let path=dir.join("game/config/iris.properties");fs::create_dir_all(path.parent().unwrap())?;
    let mut f=tempfile::NamedTempFile::new_in(path.parent().unwrap())?;f.write_all(text.as_bytes())?;f.as_file().sync_all()?;f.persist(path).map_err(|e|e.error)?;Ok(())
}
fn set_config(text:&str,file:&str,enabled:bool)->String {
    let mut lines:Vec<_>=text.lines().filter(|l|!l.trim().starts_with("shaderPack=")&&!l.trim().starts_with("enableShaders=")).map(str::to_owned).collect();
    lines.push(format!("shaderPack={}",file.replace('\\',"\\\\").replace('=',"\\=").replace(':',"\\:")));
    lines.push(format!("enableShaders={enabled}"));lines.join("\n")+"\n"
}
fn pack_path(dir:&Path,name:&str)->Result<PathBuf>{anyhow::ensure!(name.to_ascii_lowercase().ends_with(".zip"),"请选择 ZIP 光影包");blocklink_model::validate_filename(&format!("{}.jar",&name[..name.len()-4]))?;Ok(dir.join("game/shaderpacks").join(name))}
fn risk(path:&Path,mc:&str)->Result<Option<String>> {
    regular(path)?;anyhow::ensure!(fs::metadata(path)?.len()<=128*1024*1024,"光影包过大");
    let mut zip=zip::ZipArchive::new(fs::File::open(path)?)?;
    anyhow::ensure!(zip.file_names().any(|n|n.starts_with("shaders/")||n.contains("/shaders/")),"ZIP 中没有 shaders 目录");
    let properties=zip.file_names().find(|n|*n=="shaders/shaders.properties"||n.ends_with("/shaders/shaders.properties")).map(str::to_owned);
    if let Some(properties)=properties {let mut file=zip.by_name(&properties)?;
        let mut s=String::new();file.by_ref().take(2_097_153).read_to_string(&mut s)?;anyhow::ensure!(s.len()<=2_097_152,"光影描述过大");
        let old=semver::Version::parse(mc).is_ok_and(|v|v<semver::Version::new(1,21,9));
        if old && s.lines().map(str::trim).any(|l|!l.starts_with('#') && l.contains("endFlashIntensity") && (l.starts_with("variable.")||l.starts_with("uniform."))) {return Ok(Some("包含旧游戏 / Iris 不提供的 endFlashIntensity，部分光影效果可能失效".into()));}
    }
    Ok(None)
}
pub fn state(ws:&Workspace,i:&Instance)->Result<Value>{
    let dir=dirs(ws,i)?;let cfg=config(&dir)?;let active=prop(&cfg,"shaderPack");let mut packs=vec![];
    let folder=dir.join("game/shaderpacks");if folder.exists(){for e in fs::read_dir(folder)?{let e=e?;let name=e.file_name().to_string_lossy().into_owned();if name.to_ascii_lowercase().ends_with(".zip"){let result=risk(&e.path(),&i.minecraft);packs.push(json!({"file":name,"active":name==active,"warning":match result{Ok(v)=>v,Err(e)=>Some(format!("{e:#}"))}}));}}}
    packs.sort_by(|a,b|a["file"].as_str().cmp(&b["file"].as_str()));
    let mut history=vec![];for e in fs::read_dir(&dir)?{let e=e?;if e.file_name().to_string_lossy().starts_with("shader-snapshot-"){let v=read_json(&e.path())?;history.push(json!({"id":v["id"],"createdAt":v["createdAt"],"file":prop(v["config"].as_str().unwrap_or(""),"shaderPack")}));}}
    history.sort_by(|a,b|b["createdAt"].as_u64().cmp(&a["createdAt"].as_u64()));
    let log=fs::read_to_string(dir.join("game/logs/latest.log")).unwrap_or_default();
    let warnings:Vec<_>=log.lines().filter(|l|l.contains("Failed to resolve uniform")||l.contains("Shader compilation failed")||l.contains("Couldn't load the shaderpack")||l.contains("The following uniforms won't work")).take(8).collect();
    Ok(json!({"packs":packs,"active":active,"enabled":prop(&cfg,"enableShaders")=="true","iris":mods::lock(ws,i)?.mods.iter().any(|m|m.mod_id=="iris"),"plan":read_json(&dir.join("shader-plan.json")).unwrap_or(Value::Null),"history":history,"warnings":warnings}))
}
pub fn prepare(ws:&Workspace,i:&Instance,name:&str,report:game::Reporter)->Result<Value>{
    let dir=dirs(ws,i)?;write_json(&dir.join("shader-plan.json"),&Value::Null)?;let path=pack_path(&dir,name)?;let warning=risk(&path,&i.minecraft)?;
    report("识别光影包来源与兼容版本".into());let hash=hash_file(&path,"sha512")?;
    let found:Value=client()?.post("https://api.modrinth.com/v2/version_files").json(&json!({"hashes":[hash],"algorithm":"sha512"})).send()?.error_for_status()?.json()?;
    let current=found.get(&hash).context("此光影包未被 Modrinth 收录，无法可靠自动替换；可保留使用或选择其他本地光影包")?;
    anyhow::ensure!(current["files"].as_array().is_some_and(|a|a.iter().any(|v|v["hashes"]["sha512"]==hash)),"光影来源哈希不匹配");
    let project=field(current,"project_id")?;let meta=json(&format!("https://api.modrinth.com/v2/project/{project}"))?;anyhow::ensure!(meta["project_type"]=="shader","文件来源不是光影项目");
    let mut versions:Vec<Value>=client()?.get(format!("https://api.modrinth.com/v2/project/{project}/version")).query(&[("game_versions",json!([i.minecraft]).to_string()),("loaders",json!(["iris"]).to_string())]).send()?.error_for_status()?.json()?;
    versions.retain(|v|v["version_type"]=="release" && v["project_id"]==project && v["game_versions"].as_array().is_some_and(|a|a.contains(&json!(i.minecraft))) && v["loaders"].as_array().is_some_and(|a|a.contains(&json!("iris"))));
    versions.sort_by(|a,b|b["date_published"].as_str().cmp(&a["date_published"].as_str()));
    if warning.is_none(){if let Some(index)=versions.iter().position(|v|v["id"]==current["id"]){let v=versions.remove(index);versions.insert(0,v);}}
    for v in versions.into_iter().take(24){
        let files=v["files"].as_array().context("没有光影文件")?;let file=files.iter().find(|f|f["primary"]==true).or_else(||files.first()).context("没有光影文件")?;
        let filename=field(file,"filename")?;pack_path(&dir,filename)?;let sha=field(&file["hashes"],"sha512")?;blocklink_model::validate_hash(sha)?;
        let cache=ws.root().join("downloads").join(format!("{sha}.zip"));report(format!("检查光影候选 {}",v["version_number"]));download(field(file,"url")?,&cache,Some(("sha512",sha)))?;
        if risk(&cache,&i.minecraft)?.is_some(){continue;}
        let plan=json!({"id":uuid::Uuid::new_v4().to_string(),"minecraft":i.minecraft,"beforeConfig":config(&dir)?,"from":name,"fromHash":hash,"to":filename,"sha512":sha,"version":v["version_number"],"project":project,"reason":warning,"ready":true});
        write_json(&dir.join("shader-plan.json"),&plan)?;return Ok(plan);
    }
    bail!("未找到通过当前已知问题检查的正式光影版本；请保留原包或选择其他光影包")
}
fn snapshot(dir:&Path,cfg:&str)->Result<String>{let id=uuid::Uuid::new_v4().to_string();write_json(&dir.join(format!("shader-snapshot-{id}.json")),&json!({"id":id,"createdAt":auth::now(),"config":cfg}))?;Ok(id)}
pub fn apply(ws:&Workspace,i:&Instance,plan_id:&str)->Result<Value>{
    let dir=dirs(ws,i)?;let plan=read_json(&dir.join("shader-plan.json"))?;anyhow::ensure!(plan["id"]==plan_id && plan["minecraft"]==i.minecraft,"光影方案已变化，请重新检查");
    let cfg=config(&dir)?;anyhow::ensure!(plan["beforeConfig"]==cfg,"光影设置已变化，请重新检查");
    let old=pack_path(&dir,field(&plan,"from")?)?;regular(&old)?;anyhow::ensure!(hash_file(&old,"sha512")?==field(&plan,"fromHash")?,"原光影包已变化");
    let hash=field(&plan,"sha512")?;blocklink_model::validate_hash(hash)?;let cache=ws.root().join("downloads").join(format!("{hash}.zip"));
    anyhow::ensure!(hash_file(&cache,"sha512")?==hash && risk(&cache,&i.minecraft)?.is_none(),"候选光影文件校验失败");
    anyhow::ensure!(mods::lock(ws,i)?.mods.iter().any(|m|m.mod_id=="iris"),"请先在 Mods 中安装适配当前游戏的 Iris（自动安装依赖）");
    let dst=pack_path(&dir,field(&plan,"to")?)?;
    if dst.exists(){regular(&dst)?;anyhow::ensure!(hash_file(&dst,"sha512")?==hash,"同名光影包内容不同，保留原文件并停止");}else{fs::create_dir_all(dst.parent().unwrap())?;let mut f=tempfile::NamedTempFile::new_in(dst.parent().unwrap())?;std::io::copy(&mut fs::File::open(&cache)?,&mut f)?;f.as_file().sync_all()?;f.persist_noclobber(&dst).map_err(|e|e.error)?;}
    let id=snapshot(&dir,&cfg)?;write_config(&dir,&set_config(&cfg,field(&plan,"to")?,true))?;Ok(json!({"snapshotId":id,"file":plan["to"]}))
}
pub fn select(ws:&Workspace,i:&Instance,name:&str,enabled:bool)->Result<Value>{let dir=dirs(ws,i)?;pack_path(&dir,name)?;if enabled{risk(&pack_path(&dir,name)?,&i.minecraft)?;anyhow::ensure!(mods::lock(ws,i)?.mods.iter().any(|m|m.mod_id=="iris"),"请先安装 Iris");}let cfg=config(&dir)?;let id=snapshot(&dir,&cfg)?;write_config(&dir,&set_config(&cfg,name,enabled))?;Ok(json!({"snapshotId":id}))}
pub fn warnings(ws:&Workspace,i:&Instance)->Result<Vec<String>>{let dir=dirs(ws,i)?;let cfg=config(&dir)?;if prop(&cfg,"enableShaders")!="true"{return Ok(vec![]);}let file=prop(&cfg,"shaderPack");if file.is_empty(){return Ok(vec![]);}let mut warnings=vec![];if !mods::lock(ws,i)?.mods.iter().any(|m|m.mod_id=="iris"){warnings.push("已启用光影，但未找到 Iris；请检查光影页面".into());}match pack_path(&dir,&file).and_then(|p|risk(&p,&i.minecraft)){Ok(Some(w))=>warnings.push(format!("光影 {file}：{w}；可在光影页面自动适配")),Err(e)=>warnings.push(format!("光影 {file}：{e:#}")),_=>{}}Ok(warnings)}
pub fn restore(ws:&Workspace,i:&Instance,id:&str)->Result<Value>{blocklink_model::validate_uuid(id)?;let dir=dirs(ws,i)?;let old=read_json(&dir.join(format!("shader-snapshot-{id}.json")))?;let cfg=field(&old,"config")?;let file=prop(cfg,"shaderPack");if prop(cfg,"enableShaders")=="true"{regular(&pack_path(&dir,&file)?)?;}let backup=snapshot(&dir,&config(&dir)?)?;write_config(&dir,cfg)?;Ok(json!({"snapshotId":backup}))}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn detects_pack_requirement_and_preserves_settings()->Result<()> {
  let temp=tempfile::tempdir()?;let path=temp.path().join("shader.zip");let mut z=zip::ZipWriter::new(fs::File::create(&path)?);z.start_file("pack/shaders/shaders.properties",zip::write::SimpleFileOptions::default())?;z.write_all(b"    variable.float.endFlashFactor0=min(endFlashIntensity, 0.5) * 2.0")?;z.finish()?;
  assert!(risk(&path,"1.21.4")?.is_some());assert!(risk(&path,"1.21.9")?.is_none());
  let before="enableShaders=true\nshaderPack=old.zip\nmaxShadowRenderDistance=32\n";let after=set_config(before,"new.zip",true);assert_eq!(prop(&after,"shaderPack"),"new.zip");assert!(after.contains("maxShadowRenderDistance=32"));
  assert!(pack_path(temp.path(),"ComplementaryReimagined_r5.8.1.zip").is_ok());assert!(pack_path(temp.path(),"../outside.zip").is_err());let id=snapshot(temp.path(),before)?;let saved=read_json(&temp.path().join(format!("shader-snapshot-{id}.json")))?;assert_eq!(saved["config"],before);Ok(())
 }
}
