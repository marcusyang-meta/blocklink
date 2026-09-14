//! Explicitly published gameplay files, merged without replacing unrelated local settings.
use super::*;
use blocklink_model::ContentBundle;
use std::collections::{BTreeMap,BTreeSet};
const ROOTS:[&str;4]=["config","defaultconfigs","kubejs","scripts"];
fn valid(name:&str)->Result<()> {packs::portable(name)?;if !ROOTS.contains(&name.split('/').next().unwrap_or(""))||!name.contains('/') {bail!("Unsupported shared content path")};Ok(())}
pub(super) fn publish(e:&Engine,id:&str)->Result<Option<ContentBundle>> {
    if e.config(id)["shareContent"]!=true && read_json(&e.ws.instance_dir(id)?.join("published.json")).unwrap_or(Value::Null)["content"].is_null(){return Ok(None)}
    let game=e.ws.instance_dir(id)?.join("game");let mut entries=Vec::new();let mut total=0;
    if e.config(id)["shareContent"]==true {for root in ROOTS {if game.join(root).exists(){packs::export_entries(&game,root,&mut entries,&mut total)?;}}}
    if total>268_435_456 {bail!("Shared configuration exceeds 256 MiB")}
    entries.sort_by(|a,b|a.0.cmp(&b.0));
    let mut temp=tempfile::NamedTempFile::new_in(e.ws.root().join("downloads"))?;
    {let mut z=zip::ZipWriter::new(temp.as_file_mut());let options=zip::write::SimpleFileOptions::default();
    // A marker also ensures an empty bundle has a local ZIP header for the blob store.
    z.start_file("blocklink-content.json",options)?;z.write_all(b"{\"version\":1}")?;
    let mut names=BTreeSet::new();for (name,path) in entries {valid(&name)?;if !names.insert(name.to_lowercase()){bail!("Shared content filename collision")};let hash=hash_file(&path,"sha512")?;z.start_file(name,options)?;std::io::copy(&mut fs::File::open(&path)?,&mut z)?;if hash_file(&path,"sha512")?!=hash{bail!("Shared content changed while publishing")}}
    z.finish()?;}
    temp.flush()?;let blob=e.ws.import_jar(temp.path())?;
    if blob.bytes>268_435_456{bail!("Shared bundle exceeds 256 MiB")}
    Ok(Some(ContentBundle{sha512:blob.sha512,bytes:blob.bytes}))
}
pub(super) fn recover(dir:&Path)->Result<()> {
    let tx=dir.join("content-transaction");if !tx.exists(){return Ok(())}
    if tx.join("plan.json").exists()&&!tx.join("committed").exists(){
        let plan=read_json(&tx.join("plan.json"))?;
        for root in ROOTS {let current=dir.join("game").join(root);let backup=tx.join("backup").join(root);
            if backup.exists(){if current.exists(){fs::remove_dir_all(&current)?;}fs::rename(backup,current)?;}
            else if plan["original"][root]==false&&current.exists(){fs::remove_dir_all(current)?;}
        }
        write_json(&dir.join("shared-content.json"),&plan["previous"])?;
    }
    fs::remove_dir_all(tx)?;Ok(())
}
pub(super) fn apply(e:&Engine,id:&str,bundle:Option<&ContentBundle>)->Result<()> {
    apply_approved(e,id,bundle,&Value::Null)
}
fn apply_approved(e:&Engine,id:&str,bundle:Option<&ContentBundle>,approved:&Value)->Result<()> {
    let Some(bundle)=bundle else{return Ok(())};let dir=e.ws.instance_dir(id)?;recover(&dir)?;
    e.ws.verify_blob(&bundle.sha512,bundle.bytes)?;
    let previous=read_json(&dir.join("shared-content.json")).unwrap_or(json!({"files":{}}));
    let game=dir.join("game");let mut z=zip::ZipArchive::new(fs::File::open(e.ws.blob_path(&bundle.sha512)?)?)?;
    if z.len()>10001{bail!("Too many shared files")}
    let temp=tempfile::tempdir_in(&dir)?;let staged=temp.path().join("new");fs::create_dir(&staged)?;
    let mut original=json!({});for root in ROOTS {original[root]=json!(game.join(root).exists());if game.join(root).exists(){if !game.join(root).is_dir(){bail!("Shared content root must be a directory")};worlds::copy_stable(&game.join(root),&staged.join(root))?;}else{fs::create_dir(staged.join(root))?;}}
    let mut conflicts=serde_json::Map::new();
    let mut files=BTreeMap::new();let mut total=0u64;let mut spellings=BTreeSet::new();
    for n in 0..z.len(){let mut f=z.by_index(n)?;let name=f.name().to_owned();if name=="blocklink-content.json"{continue}valid(&name)?;
        if f.is_dir()||f.unix_mode().is_some_and(|m|m&0o170000==0o120000)||!spellings.insert(name.to_lowercase()){bail!("Invalid shared content entry")}
        total=total.checked_add(f.size()).context("Shared content too large")?;if total>268_435_456{bail!("Shared content exceeds 256 MiB")}
        let incoming=temp.path().join(format!("incoming-{n}"));let mut out=fs::File::create(&incoming)?;let count=std::io::copy(&mut (&mut f).take(268_435_457),&mut out)?;out.flush()?;if count!=f.size(){bail!("Corrupt shared content")}
        let hash=hash_file(&incoming,"sha512")?;let target=staged.join(&name);
        if target.exists(){if !target.is_file(){bail!("Shared content conflicts with a local directory: {name}")};let local=hash_file(&target,"sha512")?;if local!=hash&&previous["files"][&name].as_str()!=Some(&local){if approved[&name].as_str()!=Some(&local){conflicts.insert(name.clone(),json!(local));}}}
        fs::create_dir_all(target.parent().unwrap())?;fs::copy(incoming,target)?;files.insert(name,hash);
    }
    if let Some(old)=previous["files"].as_object(){for (name,hash) in old{valid(name)?;if !files.contains_key(name){let path=staged.join(name);if path.exists(){let local=hash_file(&path,"sha512")?;if local!=hash.as_str().unwrap_or("")&&approved[name].as_str()!=Some(&local){conflicts.insert(name.clone(),json!(local));};fs::remove_file(path)?;}}}}
    if !conflicts.is_empty(){
        write_json(&dir.join("content-conflicts.json"),&json!({"bundle":bundle,"files":conflicts}))?;
        bail!("本地设置与服务器不同，请在游戏页面选择如何处理。 / Local settings differ from the server. Choose how to resolve them on the game page.");
    }
    let tx=dir.join("content-transaction");fs::create_dir(&tx)?;fs::create_dir(tx.join("backup"))?;
    write_json(&tx.join("plan.json"),&json!({"original":original,"previous":previous}))?;
    let result=(||->Result<()> {for root in ROOTS{let current=game.join(root);if current.exists(){fs::rename(&current,tx.join("backup").join(root))?;}fs::rename(staged.join(root),current)?;}
        write_json(&dir.join("shared-content.json"),&json!({"sha512":bundle.sha512,"files":files}))?;fs::write(tx.join("committed"),b"ok")?;Ok(())})();
    if let Err(error)=result {recover(&dir)?;return Err(error)}recover(&dir)?;let _=fs::remove_file(dir.join("content-conflicts.json"));Ok(())
}

pub(super) fn conflicts(e:&Engine,id:&str)->Result<Value>{
    let dir=e.ws.instance_dir(id)?;
    let pending=read_json(&dir.join("content-conflicts.json")).unwrap_or(Value::Null);
    Ok(json!({"files":pending["files"].as_object().map(|m|m.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()}))
}
fn local_file(game:&Path,name:&str)->Result<std::path::PathBuf>{
    valid(name)?;let mut path=game.to_path_buf();
    for part in name.split('/') {path.push(part);let meta=fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink(){bail!("Linked settings cannot be replaced")}
        #[cfg(windows)] {use std::os::windows::fs::MetadataExt;if meta.file_attributes()&0x400!=0{bail!("Linked settings cannot be replaced")}}
    }
    if !path.is_file(){bail!("Expected a settings file")};Ok(path)
}
pub(super) fn resolve(e:&Engine,id:&str)->Result<Value>{
    e.idle(id)?;
    let dir=e.ws.instance_dir(id)?;recover(&dir)?;
    let pending=read_json(&dir.join("content-conflicts.json"))?;
    let bundle:ContentBundle=serde_json::from_value(pending["bundle"].clone())?;
    let files=pending["files"].as_object().context("No pending conflicts")?;
    if files.is_empty(){bail!("No pending conflicts")}
    // Refuse stale decisions; the user must review newly edited files again.
    for (name,hash) in files {let path=local_file(&dir.join("game"),name)?;
        if fs::symlink_metadata(&path)?.file_type().is_symlink()||hash_file(&path,"sha512")?!=hash.as_str().unwrap_or(""){bail!("Local files changed since review. Synchronize again before resolving.")}
    }
    let backup=dir.join("content-backups").join(uuid::Uuid::new_v4().to_string());fs::create_dir_all(&backup)?;
    for (name,hash) in files {
        let target=backup.join(name);fs::create_dir_all(target.parent().unwrap())?;
        fs::copy(local_file(&dir.join("game"),name)?,&target)?;
        if hash_file(&target,"sha512")?!=hash.as_str().unwrap_or(""){bail!("Local file changed while backing up; no server files were applied")}
    }
    write_json(&backup.join("backup.json"),&pending)?;
    apply_approved(e,id,Some(&bundle),&pending["files"])?;
    Ok(json!({"backup":backup,"resolved":true}))
}

#[cfg(test)] mod tests{
 use super::*;
 fn instance(e:&Engine)->Result<String>{let id=uuid::Uuid::new_v4().to_string();let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":id,"name":"Content test","minecraft":"1.21.4","loader":{"kind":"vanilla"},"runtime":{"java":"auto","memoryMiB":2048},"storage":{"linkMode":"auto"},"mods":[]}))?;e.ws.create_instance(&i)?;fs::create_dir_all(e.ws.instance_dir(&id)?.join("game"))?;Ok(id)}
 #[test]fn updates_remove_only_managed_files_and_reject_local_edits()->Result<()>{
  let temp=tempfile::tempdir()?;let e=Engine::new(temp.path())?;let host=instance(&e)?;let guest=instance(&e)?;
  e.settings.lock().unwrap()["instances"][&host]=json!({"shareContent":true});let source=e.ws.instance_dir(&host)?.join("game");fs::create_dir_all(source.join("config"))?;fs::write(source.join("config/common.toml"),b"one")?;
  let game=e.ws.instance_dir(&guest)?.join("game");fs::create_dir_all(game.join("config"))?;fs::write(game.join("config/personal.toml"),b"mine")?;
  apply(&e,&guest,publish(&e,&host)?.as_ref())?;assert_eq!(fs::read(game.join("config/common.toml"))?,b"one");
  fs::write(source.join("config/common.toml"),b"two")?;apply(&e,&guest,publish(&e,&host)?.as_ref())?;assert_eq!(fs::read(game.join("config/common.toml"))?,b"two");
  fs::write(game.join("config/common.toml"),b"edited")?;fs::write(source.join("config/common.toml"),b"three")?;assert!(apply(&e,&guest,publish(&e,&host)?.as_ref()).is_err());assert_eq!(fs::read(game.join("config/common.toml"))?,b"edited");
  fs::write(game.join("config/common.toml"),b"two")?;fs::remove_file(source.join("config/common.toml"))?;apply(&e,&guest,publish(&e,&host)?.as_ref())?;assert!(!game.join("config/common.toml").exists());assert_eq!(fs::read(game.join("config/personal.toml"))?,b"mine");Ok(())
 }
 #[test]fn resolution_preserves_edits_and_refuses_stale_choice()->Result<()>{
  let temp=tempfile::tempdir()?;let e=Engine::new(temp.path())?;let host=instance(&e)?;let guest=instance(&e)?;
  e.settings.lock().unwrap()["instances"][&host]=json!({"shareContent":true});
  let source=e.ws.instance_dir(&host)?.join("game/config");fs::create_dir_all(&source)?;fs::write(source.join("one.toml"),b"server")?;
  let target=e.ws.instance_dir(&guest)?.join("game/config");fs::create_dir_all(&target)?;fs::write(target.join("one.toml"),b"mine")?;
  let bundle=publish(&e,&host)?;assert!(apply(&e,&guest,bundle.as_ref()).is_err());
  assert_eq!(conflicts(&e,&guest)?["files"],json!(["config/one.toml"]));
  fs::write(target.join("one.toml"),b"edited again")?;assert!(resolve(&e,&guest).is_err());assert_eq!(fs::read(target.join("one.toml"))?,b"edited again");
  assert!(apply(&e,&guest,bundle.as_ref()).is_err());let result=resolve(&e,&guest)?;
  assert_eq!(fs::read(target.join("one.toml"))?,b"server");assert_eq!(fs::read(Path::new(result["backup"].as_str().unwrap()).join("config/one.toml"))?,b"edited again");assert_eq!(conflicts(&e,&guest)?["files"],json!([]));Ok(())
 }
 #[test]fn interrupted_swap_restores_original_directories()->Result<()>{
  let temp=tempfile::tempdir()?;let dir=temp.path();let tx=dir.join("content-transaction");fs::create_dir_all(tx.join("backup/config"))?;fs::write(tx.join("backup/config/old"),b"original")?;fs::create_dir_all(dir.join("game/config"))?;fs::write(dir.join("game/config/new"),b"partial")?;
  write_json(&tx.join("plan.json"),&json!({"original":{"config":true,"defaultconfigs":false,"kubejs":false,"scripts":false},"previous":{"files":{}}}))?;recover(dir)?;assert_eq!(fs::read(dir.join("game/config/old"))?,b"original");assert!(!dir.join("game/config/new").exists());Ok(())
 }
 #[test]fn rejects_traversal_and_unrelated_paths(){for path in ["../private","config/../private","saves/world/level.dat","server.properties","config/a:stream"]{assert!(valid(path).is_err())}}
}
