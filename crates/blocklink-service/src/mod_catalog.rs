use super::*;
use std::collections::HashSet;
fn entry(project:&Value)->Value {
    json!({"projectId":project["id"],"title":project["title"],"icon":project["icon_url"],"categories":project["categories"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|s|s.len()<80&&s.bytes().all(|b|b.is_ascii_lowercase()||b==b'-')).collect::<Vec<_>>(),"checkedAt":auth::now()})
}
pub fn list(e:&Engine,p:&Value)->Result<Value>{
    let i=e.ws.instance(field(p,"id")?)?;let lock=mods::lock(&e.ws,&i)?;
    let base=e.ws.root().join("mod-catalog");let mut entries=serde_json::Map::new();let mut missing=vec![];
    for m in &lock.mods {
        let cached=read_json(&base.join(format!("{}.json",m.sha512))).unwrap_or(Value::Null);
        if !cached.is_null(){entries.insert(m.sha512.clone(),cached.clone());}
        if p["refresh"]==true || cached["checkedAt"].as_u64().is_none_or(|t|auth::now().saturating_sub(t)>604800){missing.push(m.sha512.clone());}
    }
    let result=(||->Result<()> {
        if missing.is_empty(){return Ok(())}
        let c=reqwest::blocking::Client::builder().https_only(true).user_agent("Blocklink/0.1.0").connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(25)).build()?;
        for hashes in missing.chunks(100) {
            // Only content hashes are sent; no JAR, settings or world files leave the device.
            let found:Value=c.post("https://api.modrinth.com/v2/version_files").json(&json!({"hashes":hashes,"algorithm":"sha512"})).send()?.error_for_status()?.json()?;
            found.as_object().context("Mod 来源响应无效")?;
            let ids:HashSet<_>=hashes.iter().filter_map(|h|found[h]["project_id"].as_str()).collect();
            let projects:Value=if ids.is_empty(){json!([])}else{c.get("https://api.modrinth.com/v2/projects").query(&[("ids",serde_json::to_string(&ids)?)]).send()?.error_for_status()?.json()?};
            let projects=projects.as_array().context("Mod 分类响应无效")?;
            for hash in hashes {
                let value=projects.iter().find(|v|v["id"].as_str().is_some_and(|id|found[hash]["project_id"]==id)).map(entry).unwrap_or_else(||json!({"categories":[],"checkedAt":auth::now()}));
                write_json(&base.join(format!("{hash}.json")),&value)?;entries.insert(hash.clone(),value);
            }
        }Ok(())
    })();
    Ok(json!({"entries":entries,"warning":if result.is_err(){Some("暂时无法更新分类，已保留本地结果；可以稍后重试。")}else{None}}))
}
#[cfg(test)]mod tests{use super::*;
#[test]fn fresh_cache_works_without_network_and_preserves_mods()->Result<()> {
 let temp=tempfile::tempdir()?;let e=Arc::new(Engine::new(temp.path())?);
 let v=e.execute("create",&json!({"name":"catalog fixture","minecraft":"1.21.4","loader":"fabric","loaderVersion":"0.16.10","install":false}),Arc::new(|_|{}))?;let id=field(&v,"id")?;let i=e.ws.instance(id)?;
 let path=temp.path().join("fixture.jar");let mut zip=zip::ZipWriter::new(fs::File::create(&path)?);zip.start_file("fabric.mod.json",zip::write::SimpleFileOptions::default())?;zip.write_all(br#"{"schemaVersion":1,"id":"fixture","version":"1.0.0"}"#)?;zip.finish()?;
 mods::local(&e.ws,&i,&path,false)?;let before=mods::lock(&e.ws,&i)?;let hash=&before.mods[0].sha512;
 write_json(&temp.path().join("mod-catalog").join(format!("{hash}.json")),&json!({"title":"Fixture Title","categories":["optimization"],"checkedAt":auth::now()}))?;
 let result=list(&e,&json!({"id":id}))?;assert_eq!(result["entries"][hash]["categories"],json!(["optimization"]));assert!(result["warning"].is_null());assert_eq!(json!(before),json!(mods::lock(&e.ws,&i)?));assert_eq!(hash_file(&e.ws.instance_dir(id)?.join("game/mods/fixture.jar"),"sha512")?,*hash);Ok(())
}
#[test]fn project_categories_are_normalized(){let v=entry(&json!({"id":"abc","title":"Example","categories":["optimization","library",null,"unsafe:query","<script>"]}));assert_eq!(v["categories"],json!(["optimization","library"]));assert_eq!(v["title"],"Example");}
}
