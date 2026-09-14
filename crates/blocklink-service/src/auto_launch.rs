use super::*;

// This checkpoint belongs only to launch preparation. A failed preparation
// restores the previous files, while world backups remain independently usable.
fn checkpoint(e:&Engine,i:&Instance)->Result<()> {
    let dir=e.ws.instance_dir(&i.instance_id)?;
    let path=dir.join("launch-preparation.json");
    if !path.exists(){write_json(&path,&json!({"instance":i,"lock":mods::lock(&e.ws,i)?,"shaderConfig":shaders::capture(&e.ws,i)?}))?;}
    Ok(())
}
pub fn finish(e:&Engine,id:&str,restore:bool)->Result<()> {
    let path=e.ws.instance_dir(id)?.join("launch-preparation.json");
    if !path.exists(){return Ok(());}
    if restore {
        let saved=read_json(&path)?;let old:Instance=serde_json::from_value(saved["instance"].clone())?;
        let lock:Lockfile=serde_json::from_value(saved["lock"].clone())?;
        let current=e.ws.instance(id)?;mods::transition(&e.ws,&current,&old,&lock)?;
        shaders::restore_config(&e.ws,&old,field(&saved,"shaderConfig")?)?;
    }
    fs::remove_file(path)?;Ok(())
}
fn decision(plan:&Value,accepted:Option<&str>)->Option<Value>{
    if plan["disabledIds"].as_array().is_none_or(|a|a.is_empty())||accepted.is_some_and(|id|plan["id"]==id){return None;}
    let items:Vec<_>=plan["rows"].as_array().into_iter().flatten().filter(|r|r["status"]=="disable").map(|r|json!({"name":r["modId"],"reason":"这个游戏版本暂时没有可用版本"})).collect();
    Some(json!({"needsDecision":true,"kind":"mods","planId":plan["id"],"items":items,"title":"有些内容暂时无法使用","message":"关闭下列内容后可以继续准备游戏。原文件会保留，之后可以恢复。"}))
}
pub fn prepare(e:&Engine,i:&mut Instance,p:&Value,report:game::Reporter,bound:bool)->Result<Option<Value>>{
    // Do not carry an interrupted preparation into a new launch unnoticed.
    finish(e,&i.instance_id,true)?;*i=e.ws.instance(&i.instance_id)?;
    report("正在检查游戏环境".into());
    if !bound && !e.ws.instance_dir(&i.instance_id)?.join("pack.json").exists() && preflight::inspect(&e.ws,i,None)?["ready"]!=true && !matches!(i.loader,Loader::Vanilla) {
        let dir=e.ws.instance_dir(&i.instance_id)?;
        let friendly:game::Reporter={let report=report.clone();Arc::new(move |_|report("正在自动匹配游戏内容".into()))};
        let plan=if let Some(id)=p["acceptPlan"].as_str(){let plan=read_json(&dir.join("mod-update-plan.json"))?;anyhow::ensure!(plan["id"]==id,"方案已变化，请重新点击开始游戏");plan}else{mods::prepare_compatibility(&e.ws,i,false,true,&json!({}),friendly)?};
        anyhow::ensure!(plan["ready"]==true,"暂时无法自动准备此实例，请在高级管理中查看兼容结果：{}",plan["blocked"]);
        if let Some(prompt)=decision(&plan,p["acceptPlan"].as_str()){return Ok(Some(prompt));}
        report("正在备份存档".into());worlds::backup_all(e,&i.instance_id,"自动准备游戏前",&report)?;
        checkpoint(e,i)?;report("正在准备兼容的游戏内容".into());mods::apply_updates(&e.ws,i,field(&plan,"id")?)?;*i=e.ws.instance(&i.instance_id)?;
    }
    let shader=shaders::state(&e.ws,i)?;
    if shader["enabled"]==true && !shaders::warnings(&e.ws,i)?.is_empty(){
        let file=field(&shader,"active")?;let dir=e.ws.instance_dir(&i.instance_id)?;
        if let Some(token)=p["continueWithoutShader"].as_str(){
            let consent=read_json(&dir.join("launch-shader-choice.json"))?;
            anyhow::ensure!(consent["id"]==token && consent["config"]==shaders::capture(&e.ws,i)? && consent["lock"]==json!(mods::lock(&e.ws,i)?),"光影环境已变化，请重新点击开始游戏");
            checkpoint(e,i)?;shaders::select(&e.ws,i,file,false)?;
        }else {
            report("正在准备适合这个游戏的光影".into());
            let friendly:game::Reporter={let report=report.clone();Arc::new(move |_|report("正在匹配光影效果".into()))};
            let attempt=(||->Result<()>{let plan=shaders::prepare(&e.ws,i,file,friendly)?;checkpoint(e,i)?;shaders::apply(&e.ws,i,field(&plan,"id")?)?;Ok(())})();
            if let Err(error)=attempt {
                // Commit completed Mod preparation before asking about visual effects.
                finish(e,&i.instance_id,false)?;
                let token=uuid::Uuid::new_v4().to_string();write_json(&dir.join("launch-shader-choice.json"),&json!({"id":token,"config":shaders::capture(&e.ws,i)?,"lock":mods::lock(&e.ws,i)?}))?;
                return Ok(Some(json!({"needsDecision":true,"kind":"shader","planId":token,"title":"这个光影暂时无法自动准备","message":"可以先使用普通画面进入游戏，光影文件和设置快照会保留。","items":[{"name":file,"reason":"暂时无法匹配或下载"}],"details":format!("{error:#}")})));
            }
        }
    }
    preflight::require_ready(&e.ws,i,None)?;Ok(None)
}

#[cfg(test)] mod tests {
 use super::*;
 #[test] fn optional_content_requires_exact_plan_acceptance(){
  let plan=json!({"id":"new-plan","disabledIds":["example"],"rows":[{"modId":"example","status":"disable"}]});
  assert!(decision(&plan,None).is_some());assert!(decision(&plan,Some("old-plan")).is_some());assert!(decision(&plan,Some("new-plan")).is_none());
  assert!(decision(&json!({"id":"safe","disabledIds":[]}),None).is_none());
 }
 #[test] fn preparation_failure_restores_environment_and_shader_settings()->Result<()> {
  let temp=tempfile::tempdir()?;let e=Engine::new(temp.path())?;
  let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":uuid::Uuid::new_v4().to_string(),"name":"QA","minecraft":"1.21.4","loader":{"kind":"fabric","version":"0.16.10"},"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}))?;
  e.ws.create_instance(&i)?;let old=mods::lock(&e.ws,&i)?;mods::apply(&e.ws,&i,&old)?;
  shaders::restore_config(&e.ws,&i,"shaderPack=old.zip\nenableShaders=false\n")?;checkpoint(&e,&i)?;
  let mut next=i.clone();next.loader=Loader::Fabric{version:"0.19.5".into()};let mut lock=old.clone();lock.environment.loader=next.loader.clone();mods::transition(&e.ws,&i,&next,&lock)?;
  shaders::restore_config(&e.ws,&next,"shaderPack=new.zip\nenableShaders=false\n")?;finish(&e,&i.instance_id,true)?;
  assert_eq!(e.ws.instance(&i.instance_id)?.loader,i.loader);assert_eq!(shaders::capture(&e.ws,&i)?,"shaderPack=old.zip\nenableShaders=false\n");
  checkpoint(&e,&i)?;finish(&e,&i.instance_id,false)?;assert!(!e.ws.instance_dir(&i.instance_id)?.join("launch-preparation.json").exists());Ok(())
 }
}
