use super::*;
trait Catalog {
    fn versions(&mut self,project:&str)->Result<Vec<Value>>;
    fn version(&mut self,id:&str)->Result<Value>;
}
struct Live<'a>{i:&'a Instance,lists:HashMap<String,Vec<Value>>,values:HashMap<String,Value>}
impl Catalog for Live<'_> {
    fn versions(&mut self,p:&str)->Result<Vec<Value>> {
        if !self.lists.contains_key(p){let v=project_versions(p,&self.i.minecraft,self.i.loader.kind())?.as_array().context("无效候选列表")?.clone();for item in &v {self.values.insert(field(item,"id")?.into(),item.clone());}self.lists.insert(p.into(),v);}
        Ok(self.lists[p].clone())
    }
    fn version(&mut self,id:&str)->Result<Value> {
        anyhow::ensure!(!id.is_empty()&&id.bytes().all(|b|b.is_ascii_alphanumeric()),"无效版本标识");
        if !self.values.contains_key(id){self.values.insert(id.into(),json(&format!("https://api.modrinth.com/v2/version/{id}"))?);}
        Ok(self.values[id].clone())
    }
}
fn conflicts(selected:&HashMap<String,Value>)->bool {
    selected.values().any(|v|v["dependencies"].as_array().into_iter().flatten().filter(|d|d["dependency_type"]=="incompatible").any(|d|{
        if let Some(id)=d["version_id"].as_str(){selected.values().any(|v|v["id"]==id)}else{d["project_id"].as_str().is_some_and(|p|selected.contains_key(p))}
    }))
}
fn search<C:Catalog>(cat:&mut C,i:&Instance,preferred:&HashMap<String,String>,mut pending:Vec<(String,Option<String>)>,chosen:HashMap<String,Value>,budget:&mut usize)->Result<HashMap<String,Value>> {
    anyhow::ensure!(chosen.len()<129 && pending.len()<4096,"依赖组合过大");
    let Some((project,pin))=pending.pop() else{return Ok(chosen);};
    if let Some(v)=chosen.get(&project){
        anyhow::ensure!(pin.as_ref().is_none_or(|p|v["id"]==*p),"依赖要求的版本冲突：{project}");
        return search(cat,i,preferred,pending,chosen,budget);
    }
    let mut options=if let Some(pin)=pin {vec![cat.version(&pin)?]}else{
        let mut versions=cat.versions(&project)?;versions.retain(|v|v["version_type"]=="release" && supports(v,i));
        versions.sort_by(|a,b|b["date_published"].as_str().cmp(&a["date_published"].as_str()));
        versions.truncate(12);
        if let Some(prefer)=preferred.get(&project){let current=cat.version(prefer)?;versions.retain(|v|v["id"]!=*prefer);versions.insert(0,current);}
        versions
    };
    options.retain(|v|v["project_id"]==project && supports(v,i));
    let mut last=format!("{project} 没有可用的兼容候选版本");
    for v in options {
        anyhow::ensure!(*budget>0,"候选组合超过 512 次检查，请缩小变更范围或逐项选择");*budget-=1;
        let mut selected=chosen.clone();selected.insert(project.clone(),v.clone());
        if conflicts(&selected){last=format!("{project} 与其他项目声明冲突");continue;}
        let mut next=pending.clone();
        for d in v["dependencies"].as_array().into_iter().flatten().filter(|d|d["dependency_type"]=="required") {
            let pin=d["version_id"].as_str().map(str::to_owned);
            let pid=if let Some(pid)=d["project_id"].as_str(){pid.to_owned()}else if let Some(pin)=&pin {field(&cat.version(pin)?,"project_id")?.to_owned()}else{bail!("{project} 有未标明项目的外部必需依赖");};
            next.push((pid,pin));
        }
        match search(cat,i,preferred,next,selected,budget){Ok(found)=>return Ok(found),Err(e)=>last=format!("{e:#}")}
    }
    bail!("{last}")
}
pub(super) fn solve(i:&Instance,roots:&[(String,String,String)],pins:&HashSet<String>)->Result<HashMap<String,String>> {
    let preferred:HashMap<_,_>=roots.iter().map(|(_,p,v)|(p.clone(),v.clone())).collect();
    let pending=roots.iter().rev().map(|(id,p,v)|(p.clone(),pins.contains(id).then(||v.clone()))).collect();
    let mut cat=Live{i,lists:HashMap::new(),values:HashMap::new()};
    Ok(search(&mut cat,i,&preferred,pending,HashMap::new(),&mut 512)?.into_iter().map(|(p,v)|(p,v["id"].as_str().unwrap().to_owned())).collect())
}
#[cfg(test)] mod tests {
 use super::*;
 struct Fake(Vec<Value>);
 impl Catalog for Fake {fn versions(&mut self,p:&str)->Result<Vec<Value>>{Ok(self.0.iter().filter(|v|v["project_id"]==p).cloned().collect())}fn version(&mut self,id:&str)->Result<Value>{self.0.iter().find(|v|v["id"]==id).cloned().context("missing")}}
 #[test] fn backtracks_and_respects_pins()->Result<()> {
  let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":uuid::Uuid::new_v4().to_string(),"name":"QA","minecraft":"1.21.4","loader":{"kind":"fabric","version":"0.18.0"},"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}))?;
  let v=|p:&str,id:&str,date:&str,deps:Value|json!({"project_id":p,"id":id,"date_published":date,"version_type":"release","game_versions":["1.21.4"],"loaders":["fabric"],"dependencies":deps});
  let mut cat=Fake(vec![v("a","a2","2",json!([{"dependency_type":"required","project_id":"b","version_id":"b2"}])),v("a","a1","1",json!([{"dependency_type":"required","project_id":"b","version_id":"b1"}])),v("b","b1","1",json!([])),v("b","b2","2",json!([]))]);
  let pending=vec![("b".into(),Some("b1".into())),("a".into(),None)];
  let result=search(&mut cat,&i,&HashMap::new(),pending.clone(),HashMap::new(),&mut 512)?;
  assert_eq!(result["a"]["id"],"a1");
  assert!(search(&mut cat,&i,&HashMap::new(),vec![("b".into(),Some("b1".into())),("a".into(),Some("a2".into()))],HashMap::new(),&mut 512).is_err());
  assert!(search(&mut cat,&i,&HashMap::new(),pending,HashMap::new(),&mut 0).is_err());Ok(())
 }
}
