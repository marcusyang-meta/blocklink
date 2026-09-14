use anyhow::{Context,Result};
use blocklink_core::Workspace;
use blocklink_model::{Artifact,Instance,Source};
use blocklink_service::{mods,net};
use serde_json::Value;
use std::path::Path;
fn main()->Result<()> {
 let args:Vec<String>=std::env::args().collect();
 let plan:Value=net::read_json(Path::new(args.get(1).context("plan")?))?;
 let ws=Workspace::open(args.get(2).context("root")?)?;
 ws.recover_all()?;
 for row in plan.as_array().context("array")? {
  let i:Instance=serde_json::from_value(row["instance"].clone())?;
  if ws.instance(&i.instance_id).is_err() {ws.create_instance(&i)?;}
  if ws.read_lock(&i.instance_id)?.is_some() {ws.verify_instance(&i.instance_id)?;continue;}
  let mut lock=mods::lock(&ws,&i)?;
  for entry in row["mods"].as_array().context("mods")? {
   let path=Path::new(entry["path"].as_str().context("path")?);
   let (id,version,side)=match mods::inspect(path,&i.loader) {
    Ok(v)=>v,
    Err(_) => (entry["metadata"]["id"].as_str().context("fallback id")?.to_owned(),entry["metadata"]["version"].as_str().context("fallback version")?.to_owned(),serde_json::from_value(entry["metadata"]["side"].clone())?)
   };
   anyhow::ensure!(!lock.mods.iter().any(|m|m.mod_id==id),"Duplicate Mod ID {id}");
   let b=ws.import_jar(path)?;
   let source=if entry["sha512"].as_str()==Some(&b.sha512) && entry["projectId"].is_string() {
    Source::Modrinth{project_id:entry["projectId"].as_str().unwrap().into(),version_id:entry["versionId"].as_str().unwrap().into()}
   }else{Source::Local};
   lock.mods.push(Artifact{mod_id:id,version,file:path.file_name().context("filename")?.to_str().context("utf8")?.into(),sha512:b.sha512,bytes:b.bytes,side,source,dependencies:vec![]});
  }
  mods::apply(&ws,&i,&lock)?;
  ws.verify_instance(&i.instance_id)?;
  println!("{}: {} Mods imported",i.name,lock.mods.len());
 }
 Ok(())
}
