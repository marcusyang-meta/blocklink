//! Modrinth format v1. Every pack is staged before publishing an instance.
use super::*;
use blocklink_model::{Artifact, Source};
use std::collections::HashSet;

fn portable(path: &str) -> Result<()> {
    safe_join(Path::new("stage"), path)?;
    if path.is_empty() || path.len()>1024 {bail!("整合包文件路径无效")}
    for part in path.split('/') {
        let stem=part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty() || part=="." || part==".." || part.ends_with([' ','.']) || part.chars().any(|c|c.is_control()||"\\:*?\"<>|".contains(c)) || ["CON","PRN","AUX","NUL","CONIN$","CONOUT$"].contains(&stem.as_str()) || (stem.len()==4 && (stem.starts_with("COM")||stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit()) {bail!("整合包包含无法安全导入的路径：{path}")}
    }
    Ok(())
}
fn allowed(url: &str)->bool {
    reqwest::Url::parse(url).ok().is_some_and(|u|u.scheme()=="https" && u.username().is_empty() && u.password().is_none() && u.port_or_known_default()==Some(443) && matches!(u.host_str(),Some("cdn.modrinth.com"|"github.com"|"raw.githubusercontent.com"|"gitlab.com"|"objects.githubusercontent.com"|"release-assets.githubusercontent.com")))
}
fn hex(v:&Value,n:usize)->Result<&str>{let s=v.as_str().context("整合包缺少校验值")?;if s.len()!=n||!s.bytes().all(|c|c.is_ascii_hexdigit()&&!c.is_ascii_uppercase()){bail!("整合包校验值无效")}Ok(s)}
fn manifest(path:&Path)->Result<Value>{
    if fs::metadata(path)?.len()>536_870_912{bail!("整合包压缩文件超过 512 MB")}
    let mut zip=zip::ZipArchive::new(fs::File::open(path)?).context("请选择有效的整合包压缩文件")?;
    if !zip.file_names().any(|n|n=="modrinth.index.json") { return local_manifest(&mut zip); }
    let mut body=String::new();zip.by_name("modrinth.index.json").context("目前支持 Modrinth .mrpack；此文件不是该格式的整合包")?.take(8_388_609).read_to_string(&mut body)?;
    if body.len()>8_388_608{bail!("整合包清单过大")}
    let v:Value=serde_json::from_str(&body)?;
    if v["formatVersion"]!=1||v["game"]!="minecraft"{bail!("不支持此整合包格式版本")}
    field(&v,"name")?;field(&v,"versionId")?;loader(&v)?;
    let files=v["files"].as_array().context("缺少整合包文件列表")?;
    if files.len()>10000{bail!("整合包文件过多")}
    let mut names=HashSet::new();let mut bytes=0u64;
    for f in files {
        let p=field(f,"path")?;portable(p)?;
        if !names.insert(p.to_lowercase()){bail!("整合包包含重复文件：{p}")}
        hex(&f["hashes"]["sha512"],128)?;hex(&f["hashes"]["sha1"],40)?;
        let n=f["fileSize"].as_u64().context("缺少文件大小")?;
        bytes=bytes.checked_add(n).context("整合包过大")?;
        if n>2_147_483_648||bytes>8_589_934_592{bail!("整合包下载超过 8 GB 限制")}
        for side in ["client","server"] {if let Some(env)=f["env"].get(side){if !matches!(env.as_str(),Some("required"|"optional"|"unsupported")){bail!("整合包环境声明无效")}}}
        let urls=f["downloads"].as_array().context("缺少下载地址")?;
        if urls.is_empty()||!urls.iter().any(|u|u.as_str().is_some_and(allowed)){bail!("文件 {p} 没有受支持的 HTTPS 下载地址")}
    }
    Ok(v)
}
// Read the common MCBBS export used by HMCL/PCL without executing launcher
// arguments or treating an incomplete remote manifest as an installed pack.
fn local_manifest(zip:&mut zip::ZipArchive<fs::File>)->Result<Value>{
    if zip.len()>20000 {bail!("整合包文件过多")}
    let names:Vec<String>=zip.file_names().map(str::to_owned).collect();
    let metas:Vec<_>=names.iter().filter(|n|n.as_str()=="mcbbs.packmeta"||n.ends_with("/mcbbs.packmeta")).collect();
    let meta=if metas.len()==1 {metas[0].clone()} else if metas.len()>1 {bail!("压缩包内包含多个整合包，请分别导入")} else {
        let candidates:Vec<_>=names.iter().filter(|n|n.as_str()=="manifest.json"||n.ends_with("/manifest.json")).collect();
        if candidates.len()!=1 {bail!("没有找到整合包说明文件。请选择 .mrpack 或 HMCL / PCL 导出的 MCBBS ZIP 整合包")}
        candidates[0].clone()
    };
    portable(&meta)?;
    let prefix=meta.rsplit_once('/').map(|(p,_)|format!("{p}/")).unwrap_or_default();
    let mut body=String::new();zip.by_name(&meta)?.take(8_388_609).read_to_string(&mut body)?;
    if body.len()>8_388_608 {bail!("整合包说明文件过大")}
    let v:Value=serde_json::from_str(&body)?;
    if v["manifestType"]!="minecraftModpack"||v["manifestVersion"]!=1 {bail!("暂不支持此整合包格式版本")}
    if v["addons"].is_null() && !v["minecraft"].is_null() {bail!("这是 CurseForge 下载清单，目前尚未开通该来源。请使用作者提供的完整整合包，或从已安装的游戏导入")}
    let mut deps=serde_json::Map::new();
    for a in v["addons"].as_array().context("整合包缺少游戏版本信息")? {
        let key=match field(a,"id")? {"game"|"minecraft"=>"minecraft","fabric"=>"fabric-loader","quilt"=>"quilt-loader","forge"=>"forge","neoforge"=>"neoforge",other=>bail!("此整合包需要暂未支持的组件：{other}")};
        if deps.insert(key.into(),json!(field(a,"version")?)).is_some(){bail!("整合包包含重复运行组件")}
    }
    if v["libraries"].as_array().is_some_and(|a|!a.is_empty()) || ["javaArgument","launchArgument"].iter().any(|k|v["launchInfo"][k].as_array().is_some_and(|a|!a.is_empty())) {bail!("这个整合包包含自定义启动组件，暂时无法自动导入。请先在原启动器安装，再迁移游戏")}
    let layer=format!("{prefix}overrides/");
    let mut checks=vec![];let mut checked=HashSet::new();
    for f in v["files"].as_array().context("整合包缺少文件清单")? {
        if f["type"]!="addon" {bail!("这个整合包还需要从 CurseForge 下载内容。请使用作者提供的完整包，或导入已经安装好的游戏")}
        let path=field(f,"path")?;portable(path)?;
        if !checked.insert(path.to_lowercase()){bail!("整合包清单包含重复文件")}
        let hash=field(f,"hash")?.to_ascii_lowercase();
        if hash.len()!=40||!hash.bytes().all(|c|c.is_ascii_hexdigit()){bail!("整合包文件校验值无效")}
        if !names.contains(&format!("{layer}{path}")){bail!("整合包缺少文件 {path}，请下载作者提供的完整离线包")}
        checks.push(json!({"path":path,"sha1":hash}));
    }
    let mut seen=HashSet::new();let mut bytes=0u64;let mut count=0;
    for idx in 0..zip.len(){let f=zip.by_index(idx)?;if let Some(name)=f.name().strip_prefix(&layer){if f.is_dir(){continue}portable(name)?;if f.unix_mode().is_some_and(|m|m&0o170000==0o120000)||!seen.insert(name.to_lowercase()){bail!("整合包包含链接或重复文件")}
        bytes=bytes.checked_add(f.size()).context("整合包过大")?;count+=1;
        if bytes>4_294_967_296||count>10000{bail!("整合包解压超过限制")}
    }}
    let out=json!({"formatVersion":1,"game":"minecraft","name":field(&v,"name")?,"versionId":v["version"].as_str().unwrap_or("1"),"summary":v["description"],"dependencies":deps,"files":[],"_localLayer":layer,"_localChecks":checks,"_localCount":count,"_localBytes":bytes,"_memory":v["launchInfo"]["minMemory"].as_u64().unwrap_or(4096).clamp(4096,16384)});
    loader(&out)?;Ok(out)
}
fn loader(v:&Value)->Result<Loader>{
    let deps=v["dependencies"].as_object().context("整合包缺少运行环境")?;
    blocklink_model::validate_version(field(&v["dependencies"],"minecraft")?,true)?;
    let mut out=Loader::Vanilla;let mut count=0;
    for (key,val) in deps {if key=="minecraft"{continue}let version=val.as_str().context("加载器版本无效")?.to_owned();blocklink_model::validate_version(&version,true)?;
        out=match key.as_str(){"fabric-loader"=>Loader::Fabric{version},"quilt-loader"=>Loader::Quilt{version},"forge"=>Loader::Forge{version},"neoforge"=>Loader::NeoForge{version},_=>bail!("暂不支持此整合包依赖：{key}")};count+=1;
    }if count>1{bail!("整合包声明了多个加载器")}Ok(out)
}
fn included(f:&Value,optional:bool)->bool{f["env"]["client"]!="unsupported"&&(optional||f["env"]["client"]!="optional")}
pub fn preview(path:&Path)->Result<Value>{let v=manifest(path)?;let files=v["files"].as_array().unwrap();Ok(json!({"name":v["name"],"version":v["versionId"],"summary":v["summary"],"minecraft":v["dependencies"]["minecraft"],"loader":loader(&v)?,"format":if v["_localLayer"].is_string(){"MCBBS"}else{"Modrinth"},"files":v["_localCount"].as_u64().unwrap_or(files.iter().filter(|f|included(f,true)).count() as u64),"optional":files.iter().filter(|f|f["env"]["client"]=="optional").count(),"bytes":v["_localBytes"].as_u64().unwrap_or(files.iter().filter(|f|included(f,true)).map(|f|f["fileSize"].as_u64().unwrap()).sum::<u64>()),"sha512":hash_file(path,"sha512")?}))}
fn search_facets(p:&Value)->Result<Value>{
 let mut facets=vec![json!(["project_type:modpack"])];
 for (key,prefix) in [("category","categories"),("loader","categories"),("minecraft","versions")] {
  if let Some(value)=p[key].as_str().filter(|s|!s.is_empty()) {
   if value.len()>100 || !value.chars().all(|c|c.is_ascii_alphanumeric()||"._-".contains(c)){bail!("筛选条件无效")}
   facets.push(json!([format!("{prefix}:{value}")]));
  }
 }
 Ok(json!(facets))
}
pub fn categories()->Result<Value>{let tags=json("https://api.modrinth.com/v2/tag/category")?;Ok(json!(tags.as_array().context("分类列表无效")?.iter().filter(|t|t["project_type"]=="modpack").map(|t|json!({"name":t["name"]})).collect::<Vec<_>>()))}
pub fn search(p:&Value)->Result<Value>{
 let facets=search_facets(p)?.to_string();let sort=p["index"].as_str().unwrap_or("downloads");if !["downloads","relevance","updated","newest","follows"].contains(&sort){bail!("排序条件无效")}
 Ok(client()?.get("https://api.modrinth.com/v2/search").query(&[("query",p["query"].as_str().unwrap_or("")),("facets",&facets),("index",sort),("limit","24"),("offset",&p["offset"].as_u64().unwrap_or(0).min(9984).to_string())]).send()?.error_for_status()?.json()?)
}
pub fn versions(project:&str)->Result<Value>{let mut url=reqwest::Url::parse("https://api.modrinth.com/v2/project/")?;url.path_segments_mut().unwrap().pop_if_empty().push(project).push("version");json(url.as_str())}
fn fetch(url:&str,dest:&Path,hash:&str)->Result<()> {
    transfers::check()?;
    if dest.is_file() && hash_file(dest,"sha512")?==hash{return Ok(())}
    if !allowed(url){bail!("整合包下载地址不受支持")}
    let c=reqwest::blocking::Client::builder().https_only(true).user_agent("Blocklink/0.1.0").connect_timeout(Duration::from_secs(20)).timeout(Duration::from_secs(240)).redirect(reqwest::redirect::Policy::custom(|a|{if a.previous().len()>=5||!allowed(a.url().as_str()){a.error("不允许此下载重定向")}else{a.follow()}})).build()?;
    fs::create_dir_all(dest.parent().unwrap())?;let mut temp=tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
    let mut r=c.get(url).send()?.error_for_status()?;let total=r.content_length();transfers::copy(&mut r,&mut temp,total,dest.file_name().unwrap().to_str().unwrap_or("整合包文件"))?;temp.flush()?;
    if hash_file(temp.path(),"sha512")?!=hash{bail!("整合包文件校验失败")}
    temp.persist(dest).map_err(|e|e.error)?;Ok(())
}
fn overrides(pack:&Path,game:&Path)->Result<()> {
    let mut zip=zip::ZipArchive::new(fs::File::open(pack)?)?;if zip.len()>20000{bail!("整合包压缩条目过多")}
    let mut spelling=HashMap::new();
    let manifest=manifest(pack)?;
    for f in manifest["files"].as_array().unwrap(){let path=field(f,"path")?;spelling.insert(path.to_lowercase(),path.to_owned());}
    let mut total=0u64;
    let layers=if let Some(layer)=manifest["_localLayer"].as_str(){vec![layer]}else{vec!["overrides/","client-overrides/"]};
    for layer in layers {let mut names=HashSet::new();for idx in 0..zip.len(){transfers::check()?;let mut f=zip.by_index(idx)?;let Some(name)=f.name().strip_prefix(layer).map(str::to_owned) else{continue};if f.is_dir(){continue}portable(&name)?;
        if f.unix_mode().is_some_and(|m|m&0o170000==0o120000)||!names.insert(name.to_lowercase()){bail!("整合包包含链接或重复覆盖文件")}
        if spelling.get(&name.to_lowercase()).is_some_and(|old|old!=&name){bail!("整合包文件大小写冲突：{name}")}spelling.insert(name.to_lowercase(),name.clone());
        total=total.checked_add(f.size()).context("覆盖文件过大")?;if total>4_294_967_296{bail!("整合包解压超过 4 GB")}
        let dest=safe_join(game,&name)?;fs::create_dir_all(dest.parent().unwrap())?;let mut out=fs::File::create(&dest)?;
        let expected=f.size();let n=std::io::copy(&mut (&mut f).take(expected+1),&mut out)?;if n!=expected{bail!("整合包覆盖文件损坏")}
    }}
    if let Some(checks)=manifest["_localChecks"].as_array(){for f in checks{let path=field(f,"path")?;if hash_file(&safe_join(game,path)?,"sha1")?!=field(f,"sha1")?{bail!("整合包文件损坏：{path}")}}}
    Ok(())
}
pub fn install(e:&Arc<Engine>,p:&Value,report:game::Reporter)->Result<Value>{
    let pack=if let Some(path)=p["path"].as_str(){let path=PathBuf::from(path);if hash_file(&path,"sha512")?!=field(p,"sha512")?{bail!("整合包文件已改变，请重新选择")};path}else{
        report("获取整合包版本".into());let list=versions(field(p,"project")?)?;let v=list.as_array().context("版本列表无效")?.iter().find(|v|v["id"]==p["version"]).context("所选整合包版本已不可用")?;
        let files=v["files"].as_array().context("版本缺少文件")?;let f=files.iter().filter(|f|f["filename"].as_str().is_some_and(|s|s.ends_with(".mrpack"))).max_by_key(|f|f["primary"]==true).context("此版本没有 .mrpack 文件")?;
        let hash=hex(&f["hashes"]["sha512"],128)?;let path=e.ws.root().join("downloads/packs").join(format!("{hash}.mrpack"));report("下载整合包".into());fetch(field(f,"url")?,&path,hash)?;path
    };
    let i=prepare(e,p,&pack,&report)?;
    report("自动准备游戏、Java 与加载器（整合包内容已保存）".into());game::install(e.ws.root(),&i,false,None,report)?;
    Ok(json!({"id":i.instance_id,"name":i.name}))
}
fn prepare(e:&Arc<Engine>,p:&Value,pack:&Path,report:&game::Reporter)->Result<Instance>{
    let id=field(p,"newId")?;let target=e.ws.instance_dir(id)?;
    e.idle(id)?;
    let v=manifest(&pack)?;let digest=hash_file(&pack,"sha512")?;
    if !target.exists(){
        let temp=tempfile::tempdir_in(e.ws.root())?;let stage=Workspace::open(temp.path())?;
        let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":id,"name":p["name"].as_str().filter(|s|!s.trim().is_empty()).unwrap_or(field(&v,"name")?),"minecraft":v["dependencies"]["minecraft"],"loader":loader(&v)?,"runtime":{"java":"auto","memoryMiB":v["_memory"].as_u64().unwrap_or(4096)},"storage":{"linkMode":"auto"},"mods":[]}))?;
        stage.create_instance(&i)?;let game=stage.instance_dir(id)?.join("game");fs::create_dir_all(&game)?;
        let files:Vec<_>=v["files"].as_array().unwrap().iter().filter(|f|included(f,p["optional"]!=false)).collect();
        for (index,f) in files.iter().enumerate(){transfers::check()?;let name=field(f,"path")?;report(format!("准备内容 {}/{} · {name}",index+1,files.len()));let hash=hex(&f["hashes"]["sha512"],128)?;let cache=e.ws.root().join("downloads/pack-files").join(hash);let mut error=None;
            for url in f["downloads"].as_array().unwrap().iter().filter_map(Value::as_str).filter(|u|allowed(u)){match fetch(url,&cache,hash){Ok(())=>{error=None;break},Err(err)=>{if err.is::<transfers::Cancelled>(){return Err(err)}error=Some(err)}}}if let Some(err)=error{return Err(err)}
            if fs::metadata(&cache)?.len()!=f["fileSize"].as_u64().unwrap()||hash_file(&cache,"sha1")?!=field(&f["hashes"],"sha1")?{bail!("文件大小或 SHA-1 校验失败：{name}")}
            let dest=safe_join(&game,name)?;fs::create_dir_all(dest.parent().unwrap())?;fs::copy(cache,dest)?;
        }
        report("导入配置、资源包与光影".into());overrides(&pack,&game)?;
        let mut lock=mods::lock(&stage,&i)?;let mut ids=HashSet::new();let moddir=game.join("mods");if moddir.is_dir(){for entry in fs::read_dir(&moddir)?{transfers::check()?;let path=entry?.path();if !path.is_file()||path.extension().and_then(|x|x.to_str())!=Some("jar"){continue}
            let (mod_id,version,side)=mods::inspect(&path,&i.loader).with_context(||format!("无法导入 {}",path.display()))?;if !ids.insert(mod_id.clone()){bail!("整合包包含重复模组：{mod_id}")}
            let blob=e.ws.import_jar(&path)?;let dest=stage.blob_path(&blob.sha512)?;fs::create_dir_all(dest.parent().unwrap())?;if !dest.exists(){if fs::hard_link(e.ws.blob_path(&blob.sha512)?,&dest).is_err(){fs::copy(e.ws.blob_path(&blob.sha512)?,&dest)?;}}
            lock.mods.push(Artifact{mod_id,version,file:path.file_name().unwrap().to_string_lossy().into_owned(),sha512:blob.sha512,bytes:blob.bytes,side,source:Source::Local,dependencies:vec![]});fs::remove_file(path)?;
        }}mods::apply(&stage,&i,&lock)?;
        write_json(&stage.instance_dir(id)?.join("pack.json"),&json!({"sha512":digest,"name":v["name"],"version":v["versionId"],"project":p["project"],"versionId":p["version"]}))?;
        transfers::check()?;report("保存整合包实例".into());fs::rename(stage.instance_dir(id)?,&target)?;
    }else if read_json(&target.join("pack.json"))?["sha512"]!=digest{bail!("重试目标与整合包不一致")}
    {let mut settings=e.settings.lock().unwrap();if settings["instances"][id].is_null(){settings["instances"][id]=json!({"server":false,"port":25565,"javaPath":"","serverId":""});}}e.save_settings()?;
    Ok(e.ws.instance(id)?)
}

#[cfg(test)]mod tests{use super::*;
fn local_fixture(path:&Path,mut v:Value,entries:&[(&str,&[u8])])->Result<()>{
 if v.is_null(){v=json!({"manifestType":"minecraftModpack","manifestVersion":1,"name":"国内整合包","version":"2","addons":[{"id":"game","version":"1.21.4"},{"id":"fabric","version":"0.16.10"}],"files":[]})}
 let mut z=zip::ZipWriter::new(fs::File::create(path)?);z.start_file("mcbbs.packmeta",zip::write::SimpleFileOptions::default())?;z.write_all(v.to_string().as_bytes())?;
 for (name,bytes) in entries{z.start_file(*name,zip::write::SimpleFileOptions::default())?;z.write_all(bytes)?;}z.finish()?;Ok(())
}
#[test]fn mcbbs_import_preserves_content_and_auto_environment()->Result<()>{
 let temp=tempfile::tempdir()?;let pack=temp.path().join("domestic.zip");local_fixture(&pack,Value::Null,&[("overrides/config/测试.txt",b"config"),("overrides/shaderpacks/example.zip",b"shader"),("launcher.exe",b"unused")])?;
 let p=preview(&pack)?;assert_eq!(p["format"],"MCBBS");assert_eq!(p["files"],2);assert_eq!(p["loader"]["kind"],"fabric");
 let e=Arc::new(Engine::new(&temp.path().join("data"))?);let id=uuid::Uuid::new_v4().to_string();let report:game::Reporter=Arc::new(|_|{});let i=prepare(&e,&json!({"newId":id}),&pack,&report)?;
 assert_eq!(i.minecraft,"1.21.4");assert_eq!(fs::read(e.ws.instance_dir(&id)?.join("game/config/测试.txt"))?,b"config");assert!(!e.ws.instance_dir(&id)?.join("game/launcher.exe").exists());Ok(())
}
#[test]fn mcbbs_rejects_incomplete_unsafe_and_corrupt_packs()->Result<()>{
 let temp=tempfile::tempdir()?;let pack=temp.path().join("bad.zip");
 for entries in [vec![("overrides/../escape",&b"bad"[..])],vec![("overrides/config/A.txt",&b"a"[..]),("overrides/config/a.txt",&b"b"[..])]]{local_fixture(&pack,Value::Null,&entries)?;assert!(preview(&pack).is_err());}
 let mut v=json!({"manifestType":"minecraftModpack","manifestVersion":1,"name":"test","addons":[{"id":"game","version":"26.2"},{"id":"neoforge","version":"26.2.0"}],"files":[{"type":"addon","path":"config/a.txt","hash":"0000000000000000000000000000000000000000"}]});
 local_fixture(&pack,v.clone(),&[])?;assert!(preview(&pack).is_err());
 local_fixture(&pack,v.clone(),&[("overrides/config/a.txt",b"wrong")])?;let e=Arc::new(Engine::new(&temp.path().join("data"))?);let id=uuid::Uuid::new_v4().to_string();let report:game::Reporter=Arc::new(|_|{});assert!(prepare(&e,&json!({"newId":id}),&pack,&report).is_err());assert!(!e.ws.instance_dir(&id)?.exists());
 v["files"]=json!([{"type":"curse","projectID":1,"fileID":2}]);local_fixture(&pack,v,&[])?;assert!(preview(&pack).unwrap_err().to_string().contains("CurseForge"));Ok(())
}
fn fixture(path:&Path,entries:&[(&str,Vec<u8>)])->Result<()>{let mut z=zip::ZipWriter::new(fs::File::create(path)?);z.start_file("modrinth.index.json",zip::write::SimpleFileOptions::default())?;z.write_all(serde_json::to_string(&json!({"formatVersion":1,"game":"minecraft","name":"Fixture","versionId":"1","dependencies":{"minecraft":"1.21.4","fabric-loader":"0.16.10"},"files":[]}))?.as_bytes())?;for (name,bytes) in entries{z.start_file(*name,zip::write::SimpleFileOptions::default())?;z.write_all(bytes)?;}z.finish()?;Ok(())}
#[test]fn import_is_atomic_and_reuses_mods()->Result<()>{
 let root=tempfile::tempdir()?;let e=Arc::new(Engine::new(&root.path().join("data"))?);let jar=root.path().join("example.jar");let mut z=zip::ZipWriter::new(fs::File::create(&jar)?);z.start_file("fabric.mod.json",zip::write::SimpleFileOptions::default())?;z.write_all(br#"{"schemaVersion":1,"id":"example","version":"1.0.0","environment":"*"}"#)?;z.finish()?;
 let pack=root.path().join("good.mrpack");fixture(&pack,&[("overrides/mods/example.jar",fs::read(&jar)?),("overrides/config/example.txt",b"common".to_vec()),("client-overrides/config/example.txt",b"client".to_vec()),("server-overrides/server.properties",b"server-only".to_vec())])?;
 let report:game::Reporter=Arc::new(|_|{});let a=uuid::Uuid::new_v4().to_string();let b=uuid::Uuid::new_v4().to_string();
 for id in [&a,&b]{let i=prepare(&e,&json!({"newId":id}),&pack,&report)?;let game=e.ws.instance_dir(id)?.join("game");assert_eq!(fs::read(game.join("config/example.txt"))?,b"client");assert!(!game.join("server.properties").exists());assert_eq!(mods::lock(&e.ws,&i)?.mods.len(),1);assert_eq!(fs::read(game.join("mods/example.jar"))?,fs::read(&jar)?);}
 let ia=e.ws.instance(&a)?;let ib=e.ws.instance(&b)?;assert_eq!(mods::lock(&e.ws,&ia)?.mods[0].sha512,mods::lock(&e.ws,&ib)?.mods[0].sha512);
 let bad=root.path().join("bad.mrpack");fixture(&bad,&[("overrides/../escape",b"no".to_vec())])?;let id=uuid::Uuid::new_v4().to_string();assert!(prepare(&e,&json!({"newId":id}),&bad,&report).is_err());assert!(!e.ws.instance_dir(&id)?.exists());assert_eq!(e.ws.instances()?.len(),2);Ok(())
}
#[test]fn override_traversal_and_case_collision_rejected()->Result<()>{let temp=tempfile::tempdir()?;for entries in [vec![("overrides/../outside",b"bad".to_vec())],vec![("overrides/config/A.txt",vec![]),("overrides/config/a.txt",vec![])]]{let pack=temp.path().join("bad.mrpack");fixture(&pack,&entries)?;assert!(overrides(&pack,&temp.path().join("game")).is_err());}assert!(!temp.path().join("outside").exists());Ok(())}
#[test]fn combined_filters_are_anded(){assert_eq!(search_facets(&json!({"category":"adventure","loader":"fabric","minecraft":"26.2"})).unwrap(),json!([["project_type:modpack"],["categories:adventure"],["categories:fabric"],["versions:26.2"]]));assert!(search_facets(&json!({"category":"x OR y"})).is_err());}
#[test]fn portable_paths(){for p in ["../x","/x","a/../x","CON.txt","config/a.","config/a:b","a//b","a\\b"]{assert!(portable(p).is_err(),"{p}")}assert!(portable("config/sodium-options.json").is_ok());}
#[test]fn client_selection(){assert!(!included(&json!({"env":{"client":"unsupported"}}),true));assert!(!included(&json!({"env":{"client":"optional"}}),false));assert!(included(&json!({}),false));}
#[test]fn download_hosts(){assert!(allowed("https://cdn.modrinth.com/data/x"));for u in ["http://cdn.modrinth.com/a","https://127.0.0.1/a","https://cdn.modrinth.com.evil.org/a","https://a@cdn.modrinth.com/a"]{assert!(!allowed(u));}}
}
