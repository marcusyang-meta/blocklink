mod solver;
use crate::{game::Reporter, net::*};
use anyhow::{bail, Context, Result};
use blocklink_core::Workspace;
use blocklink_model::{
    Artifact, Dependency, Environment, Instance, LinkMode, Loader, Lockfile, Side, Source,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
};
pub fn lock(ws: &Workspace, i: &Instance) -> Result<Lockfile> {
    Ok(ws.read_lock(&i.instance_id)?.unwrap_or(Lockfile {
        schema_version: 1,
        environment: Environment {
            minecraft: i.minecraft.clone(),
            loader: i.loader.clone(),
        },
        mods: vec![],
    }))
}
pub fn inspect(path: &Path, loader: &Loader) -> Result<(String, String, Side)> {
    let mut ar = zip::ZipArchive::new(fs::File::open(path)?)?;
    if matches!(loader, Loader::NeoForge { .. } | Loader::Forge { .. }) {
        let neo = matches!(loader, Loader::NeoForge { .. });
        let modern = neo && ar.file_names().any(|n| n == "META-INF/neoforge.mods.toml");
        let mut text = String::new();
        ar.by_name(if modern {
            "META-INF/neoforge.mods.toml"
        } else {
            "META-INF/mods.toml"
        })
        .context("此文件缺少当前 Loader 所需的 Mods TOML 描述")?
        .take(1_048_577)
        .read_to_string(&mut text)?;
        if text.len() > 1_048_576 {
            bail!("Mod 描述过大")
        }
        let meta: toml::Value = toml::from_str(&text)?;
        if neo
            && !modern
            && !meta
                .get("dependencies")
                .and_then(toml::Value::as_table)
                .is_some_and(|t| {
                    t.values().any(|v| {
                        v.as_array().is_some_and(|a| {
                            a.iter().any(|d| {
                                d.get("modId").and_then(toml::Value::as_str) == Some("neoforge")
                            })
                        })
                    })
                })
        {
            bail!("无法确认此 JAR 支持 NeoForge，不能将 Forge Mod 当作 NeoForge Mod 导入")
        }
        if !neo {
            let dependencies = meta.get("dependencies").and_then(toml::Value::as_table);
            let has = |name: &str| {
                dependencies.is_some_and(|t| {
                    t.values().any(|v| {
                        v.as_array().is_some_and(|a| {
                            a.iter()
                                .any(|d| d.get("modId").and_then(toml::Value::as_str) == Some(name))
                        })
                    })
                })
            };
            if has("neoforge") && !has("forge") {
                bail!("此文件只声明了 NeoForge 支持，不能导入 Forge 实例")
            }
        }
        let m = meta
            .get("mods")
            .and_then(toml::Value::as_array)
            .and_then(|a| a.first())
            .context("缺少 Mod 声明")?;
        let id = m
            .get("modId")
            .and_then(toml::Value::as_str)
            .context("缺少 modId")?
            .to_owned();
        let mut version = m
            .get("version")
            .and_then(toml::Value::as_str)
            .unwrap_or("1")
            .to_owned();
        if version.contains("${") {
            let mut manifest = String::new();
            ar.by_name("META-INF/MANIFEST.MF")?
                .take(65537)
                .read_to_string(&mut manifest)?;
            version = manifest
                .lines()
                .find_map(|l| l.strip_prefix("Implementation-Version: "))
                .context("无法读取 Mod 版本")?
                .trim()
                .into();
        }
        blocklink_model::validate_mod_id(&id)?;
        blocklink_model::validate_version(&version, true)?;
        return Ok((id, version, Side::Both));
    }
    if matches!(loader, Loader::Quilt { .. }) && ar.file_names().any(|n| n == "quilt.mod.json") {
        let mut text = String::new();
        ar.by_name("quilt.mod.json")?
            .take(1_048_577)
            .read_to_string(&mut text)?;
        if text.len() > 1_048_576 {
            bail!("Mod 描述过大")
        }
        let meta = parse_mod_json(&text)?;
        let id = field(&meta["quilt_loader"], "id")?.to_owned();
        let version = field(&meta["quilt_loader"], "version")?.to_owned();
        blocklink_model::validate_mod_id(&id)?;
        blocklink_model::validate_version(&version, true)?;
        let side = match meta["minecraft"]["environment"].as_str() {
            Some("client") => Side::Client,
            Some("dedicated_server") | Some("server") => Side::Server,
            _ => Side::Both,
        };
        return Ok((id, version, side));
    }
    if !matches!(loader, Loader::Fabric { .. } | Loader::Quilt { .. }) {
        bail!("原版实例不支持 Mod")
    }
    let f = ar
        .by_name("fabric.mod.json")
        .context("此文件不是 Fabric Mod：缺少 fabric.mod.json")?;
    let mut text = String::new();
    f.take(1_048_577).read_to_string(&mut text)?;
    if text.len() > 1_048_576 {
        bail!("Mod 描述过大")
    }
    let meta = parse_mod_json(&text)?;
    let id = field(&meta, "id")?.to_owned();
    let version = field(&meta, "version")?.to_owned();
    blocklink_model::validate_mod_id(&id)?;
    blocklink_model::validate_version(&version, true)?;
    let side = match meta["environment"].as_str() {
        Some("client") => Side::Client,
        Some("server") => Side::Server,
        _ => Side::Both,
    };
    Ok((id, version, side))
}
pub fn local(ws: &Workspace, i: &Instance, path: &Path, server: bool) -> Result<Value> {
    if matches!(i.loader, Loader::Vanilla) {
        bail!("请先创建带 Mod Loader 的实例")
    }
    let (id, version, side) = inspect(path, &i.loader)?;
    if server && side == Side::Client {
        bail!("客户端专用 Mod 不能加入服务器")
    }
    let file = path
        .file_name()
        .and_then(|x| x.to_str())
        .context("文件名无效")?
        .to_owned();
    blocklink_model::validate_filename(&file)?;
    let b = ws.import_jar(path)?;
    let mut lock = lock(ws, i)?;
    lock.mods.retain(|m| m.mod_id != id);
    lock.mods.push(Artifact {
        mod_id: id,
        version,
        file,
        sha512: b.sha512,
        bytes: b.bytes,
        side,
        source: Source::Local,
        dependencies: vec![],
    });
    apply(ws, i, &lock)
}
pub fn apply(ws: &Workspace, i: &Instance, lock: &Lockfile) -> Result<Value> {
    let receipt = ws.sync(&i.instance_id, lock, LinkMode::Auto)?;
    let mut intent = i.clone();
    intent.mods = lock
        .mods
        .iter()
        .map(|m| match &m.source {
            Source::Modrinth {
                project_id,
                version_id,
            } => blocklink_model::ModRequest::Registry(blocklink_model::RegistryRequest {
                mod_id: m.mod_id.clone(),
                project_id: project_id.clone(),
                version: version_id.clone(),
                side: m.side,
            }),
            _ => blocklink_model::ModRequest::Local(blocklink_model::LocalRequest {
                mod_id: m.mod_id.clone(),
                sha512: m.sha512.clone(),
                side: m.side,
            }),
        })
        .collect();
    write_json(
        &ws.instance_dir(&i.instance_id)?.join("instance.json"),
        &intent,
    )?;
    Ok(serde_json::to_value(receipt)?)
}
pub fn search(query: &str, minecraft: &str, loader: &str, options: &Value) -> Result<Value> {
    check_loader(loader)?;
    let mut facets = json!([
        ["project_type:mod"],
        [format!("categories:{loader}")],
        [format!("versions:{minecraft}")]
    ]);
    let category = options["category"].as_str().unwrap_or("");
    if !category.is_empty() {
        if !["optimization", "utility", "adventure", "decoration", "technology", "magic", "worldgen", "storage", "equipment", "mobs", "food", "transportation", "library", "social", "management", "game-mechanics"].contains(&category) {
            bail!("无效的 Mod 分类");
        }
        facets.as_array_mut().unwrap().push(json!([format!("categories:{category}")]));
    }
    let index = options["index"].as_str().unwrap_or("relevance");
    if !["relevance", "downloads", "follows", "newest", "updated"].contains(&index) {
        bail!("无效的排序方式");
    }
    let offset = options["offset"].as_u64().unwrap_or(0).min(10000).to_string();
    Ok(client()?
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query),
            ("facets", &facets.to_string()),
            ("limit", "24"),
            ("index", index),
            ("offset", &offset),
        ])
        .send()?
        .error_for_status()?
        .json()?)
}
fn check_loader(loader: &str) -> Result<()> {
    if !["fabric", "neoforge", "forge", "quilt"].contains(&loader) {
        bail!("不支持的 Mod Loader")
    }
    Ok(())
}
fn project_versions(project: &str, mc: &str, loader: &str) -> Result<Value> {
    check_loader(loader)?;
    if !project
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        bail!("项目 ID 无效")
    }
    Ok(client()?
        .get(format!(
            "https://api.modrinth.com/v2/project/{project}/version"
        ))
        .query(&[
            ("loaders", json!([loader]).to_string()),
            ("game_versions", json!([mc]).to_string()),
        ])
        .send()?
        .error_for_status()?
        .json()?)
}
pub fn versions(project: &str, mc: &str, loader: &str) -> Result<Value> {
    project_versions(project, mc, loader)
}
struct Resolver<'a> {
    ws: &'a Workspace,
    i: &'a Instance,
    server: bool,
    report: Reporter,
    visiting: HashSet<String>,
    resolved: HashMap<String, Artifact>,
    projects: HashMap<String, String>,
    stable_only: bool,
    incompatible: Vec<(String, String, String)>,
    choices: HashMap<String,String>,
}
impl Resolver<'_> {
    fn resolve(&mut self, project: &str, version: Option<&str>) -> Result<Artifact> {
        if self.resolved.len() > 128 || self.visiting.len() > 128 {
            bail!("依赖树过大")
        }
        let selected=version.map(str::to_owned).or_else(||self.choices.get(project).cloned());
        let v = if let Some(v) = selected.as_deref() {
            if !v.bytes().all(|c| c.is_ascii_alphanumeric()) {
                bail!("版本 ID 无效")
            };
            json(&format!("https://api.modrinth.com/v2/version/{v}"))?
        } else {
            project_versions(project, &self.i.minecraft, self.i.loader.kind())?
                .as_array()
                .and_then(|v| if self.stable_only {v.iter().filter(|v| v["version_type"] == "release").max_by_key(|v| v["date_published"].as_str().unwrap_or(""))} else {v.first()})
                .cloned()
                .context("没有兼容此游戏版本的 Mod")?
        };
        let vid = field(&v, "id")?.to_owned();
        let pid = field(&v, "project_id")?.to_owned();
        if !project.is_empty() && pid != project {
            let p = json(&format!("https://api.modrinth.com/v2/project/{project}"))?;
            if p["id"] != pid {
                bail!("Mod 版本不属于所选项目")
            }
        }
        if !v["game_versions"]
            .as_array()
            .is_some_and(|a| a.contains(&json!(self.i.minecraft)))
            || !v["loaders"]
                .as_array()
                .is_some_and(|a| a.contains(&json!(self.i.loader.kind())))
        {
            bail!("Mod 与实例版本或 Loader 不兼容")
        }
        if let Some(a) = self.resolved.get(&vid) {
            return Ok(a.clone());
        }
        if let Some(other) = self.projects.insert(pid.clone(), vid.clone()) {
            if other != vid {
                bail!("依赖版本冲突：{pid}")
            }
        }
        if !self.visiting.insert(vid.clone()) {
            bail!("检测到循环依赖：{pid}")
        }
        (self.report)(format!("下载 Mod · {}", v["name"].as_str().unwrap_or(&pid)));
        let files = v["files"].as_array().context("Mod 没有下载文件")?;
        let file = files
            .iter()
            .find(|f| f["primary"] == true)
            .or_else(|| files.first())
            .context("Mod 文件为空")?;
        let name = field(file, "filename")?;
        blocklink_model::validate_filename(name)?;
        let hash = field(&file["hashes"], "sha512")?;
        blocklink_model::validate_hash(hash)?;
        let path = self.ws.root().join("downloads").join(format!("{hash}.jar"));
        download(field(file, "url")?, &path, Some(("sha512", hash)))?;
        let (id, version, mut side) = inspect(&path, &self.i.loader)?;
        let project_meta = json(&format!("https://api.modrinth.com/v2/project/{pid}"))?;
        if project_meta["server_side"] == "unsupported" {
            side = Side::Client
        }
        if project_meta["client_side"] == "unsupported" {
            side = Side::Server
        }
        if self.server && side == Side::Client {
            bail!("{} 是客户端专用 Mod", id)
        }
        let blob = self.ws.import_jar(&path)?;
        if self.stable_only {
            check_fabric_loader(&path,&self.i.loader)?;
            for d in v["dependencies"].as_array().into_iter().flatten().filter(|d|d["dependency_type"]=="incompatible") {
                self.incompatible.push((pid.clone(),d["project_id"].as_str().unwrap_or("").into(),d["version_id"].as_str().unwrap_or("").into()));
            }
        }
        let mut dependencies = vec![];
        for d in v["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["dependency_type"] == "required")
        {
            let a = self.resolve(
                d["project_id"].as_str().unwrap_or(""),
                d["version_id"].as_str(),
            )?;
            dependencies.push(Dependency {
                mod_id: a.mod_id,
                version: a.version,
            });
        }
        let a = Artifact {
            mod_id: id,
            version,
            file: name.into(),
            sha512: blob.sha512,
            bytes: blob.bytes,
            side,
            source: Source::Modrinth {
                project_id: pid,
                version_id: vid.clone(),
            },
            dependencies,
        };
        self.visiting.remove(&vid);
        self.resolved.insert(vid, a.clone());
        Ok(a)
    }
}
pub fn add(
    ws: &Workspace,
    i: &Instance,
    project: &str,
    version: Option<&str>,
    server: bool,
    report: Reporter,
) -> Result<Value> {
    if matches!(i.loader, Loader::Vanilla) {
        bail!("Vanilla 不支持 Mod，请创建带 Mod Loader 的实例")
    }
    let mut r = Resolver {
        ws,
        i,
        server,
        report,
        visiting: HashSet::new(),
        resolved: HashMap::new(),
        projects: HashMap::new(), stable_only: false, incompatible: Vec::new(), choices: HashMap::new(),
    };
    r.resolve(project, version)?;
    let mut lock = lock(ws, i)?;
    for a in r.resolved.into_values() {
        lock.mods.retain(|m| m.mod_id != a.mod_id);
        lock.mods.push(a)
    }
    lock.mods.sort_by(|a, b| a.mod_id.cmp(&b.mod_id));
    apply(ws, i, &lock)
}
pub fn remove(ws: &Workspace, i: &Instance, id: &str) -> Result<Value> {
    let mut l = lock(ws, i)?;
    if let Some(m) = l
        .mods
        .iter()
        .find(|m| m.dependencies.iter().any(|d| d.mod_id == id))
    {
        bail!("{} 依赖此 Mod，请先移除它", m.mod_id)
    }
    l.mods.retain(|m| m.mod_id != id);
    apply(ws, i, &l)
}

// Update preparation never changes an instance: downloaded artifacts only enter the shared store.
pub fn prepare_updates(ws: &Workspace, i: &Instance, server: bool, report: Reporter) -> Result<Value> {
    let before = lock(ws, i)?;
    let mut resolver = Resolver { ws, i, server, report: report.clone(), visiting: HashSet::new(), resolved: HashMap::new(), projects: HashMap::new(), stable_only: false, incompatible: Vec::new(), choices: HashMap::new() };
    let mut targets = Vec::new();
    let mut skipped = Vec::new();
    for artifact in &before.mods {
        if let Source::Modrinth { project_id, version_id } = &artifact.source {
            report(format!("检查更新 · {}", artifact.mod_id));
            let versions = project_versions(project_id, &i.minecraft, i.loader.kind())?;
            let versions = versions.as_array().context("版本列表无效")?;
            let current = versions.iter().find(|v| v["id"] == *version_id);
            let latest = versions.iter().filter(|v| v["version_type"] == "release").max_by_key(|v| v["date_published"].as_str().unwrap_or(""));
            if let (Some(current), Some(latest)) = (current, latest) {
                if latest["id"] != *version_id && latest["date_published"].as_str() > current["date_published"].as_str() {
                    targets.push((project_id.clone(), field(latest,"id")?.to_owned()));
                }
            } else { skipped.push(artifact.mod_id.clone()); }
        } else { skipped.push(artifact.mod_id.clone()); }
    }
    for (project, version) in targets { resolver.resolve(&project, Some(&version))?; }
    let mut after = before.clone();
    for artifact in resolver.resolved.into_values() {
        if before.mods.iter().any(|m| m.mod_id == artifact.mod_id && !matches!(m.source, Source::Modrinth {..})) {
            bail!("更新与本地导入 Mod {} 冲突，请手动处理", artifact.mod_id);
        }
        after.mods.retain(|m| m.mod_id != artifact.mod_id);
        after.mods.push(artifact);
    }
    after.mods.sort_by(|a,b| a.mod_id.cmp(&b.mod_id));
    i.accepts(&after)?;
    let changes: Vec<_> = after.mods.iter().filter_map(|a| {
        let old = before.mods.iter().find(|m| m.mod_id == a.mod_id);
        if old.is_some_and(|m| m.sha512 == a.sha512) { return None; }
        Some(json!({"modId":a.mod_id,"from":old.map(|m| &m.version),"to":a.version,"bytes":a.bytes,"dependency":old.is_none()}))
    }).collect();
    let plan = json!({"id":uuid::Uuid::new_v4().to_string(),"createdAt":crate::auth::now(),"before":before,"after":after,"changes":changes,"skipped":skipped});
    write_json(&ws.instance_dir(&i.instance_id)?.join("mod-update-plan.json"), &plan)?;
    Ok(plan)
}

pub fn update_state(ws: &Workspace, i: &Instance) -> Result<Value> {
    let dir = ws.instance_dir(&i.instance_id)?;
    let mut history = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry=entry?;
        let name=entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("mod-snapshot-") && name.ends_with(".json") {
            let value=read_json(&entry.path())?;
            history.push(json!({"id":value["id"],"createdAt":value["createdAt"],"reason":value["reason"],"count":value["lock"]["mods"].as_array().map_or(0,Vec::len)}));
        }
    }
    history.sort_by(|a,b| b["createdAt"].as_u64().cmp(&a["createdAt"].as_u64()));
    let file=dir.join("mod-update-plan.json");
    Ok(json!({"plan":if file.exists(){read_json(&file)?}else{Value::Null},"history":history,"disabled":disabled(ws,i)?}))
}

fn disabled(ws: &Workspace, i: &Instance) -> Result<Vec<Artifact>> {
    let active=lock(ws,i)?;
    let mut result=Vec::new();
    for entry in fs::read_dir(ws.instance_dir(&i.instance_id)?)? {
        let entry=entry?;
        let name=entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("mod-disabled-") && name.ends_with(".json") {
            let artifact: Artifact=serde_json::from_value(read_json(&entry.path())?)?;
            artifact.validate()?;
            if !active.mods.iter().any(|m| m.mod_id==artifact.mod_id) {result.push(artifact);}
        }
    }
    Ok(result)
}

pub fn toggle(ws: &Workspace, i: &Instance, mod_id: &str, enable: bool) -> Result<Value> {
    blocklink_model::validate_mod_id(mod_id)?;
    let mut target=lock(ws,i)?;
    let file=ws.instance_dir(&i.instance_id)?.join(format!("mod-disabled-{mod_id}.json"));
    if enable {
        if target.mods.iter().any(|m| m.mod_id==mod_id){bail!("此 Mod 已启用");}
        let artifact: Artifact=serde_json::from_value(read_json(&file)?)?;
        if artifact.mod_id!=mod_id {bail!("停用记录不匹配");}
        target.mods.push(artifact);
        i.accepts(&target)?;
        snapshot(ws,i,"启用前")?;
        apply(ws,i,&target)?;
        fs::remove_file(file)?;
    } else {
        let artifact=target.mods.iter().find(|m| m.mod_id==mod_id).context("找不到此 Mod")?.clone();
        target.mods.retain(|m| m.mod_id!=mod_id);
        i.accepts(&target).context("其他 Mod 依赖此文件，请先停用依赖它的 Mod")?;
        snapshot(ws,i,"停用前")?;
        // Save recovery metadata first. If sync fails, the still-active Mod is excluded from disabled().
        write_json(&file,&artifact)?;
        apply(ws,i,&target)?;
    }
    Ok(json!({"enabled":enable,"modId":mod_id}))
}

fn snapshot(ws: &Workspace, i: &Instance, reason: &str) -> Result<String> {
    let id=uuid::Uuid::new_v4().to_string();
    let value=json!({"id":id,"createdAt":crate::auth::now(),"reason":reason,"instance":i,"lock":lock(ws,i)?});
    write_json(&ws.instance_dir(&i.instance_id)?.join(format!("mod-snapshot-{id}.json")),&value)?;
    Ok(id)
}

pub fn apply_updates(ws: &Workspace, i: &Instance, plan_id: &str) -> Result<Value> {
    let plan=read_json(&ws.instance_dir(&i.instance_id)?.join("mod-update-plan.json"))?;
    if plan["id"] != plan_id { bail!("更新清单已改变，请重新检查"); }
    if serde_json::to_value(lock(ws,i)?)? != plan["before"] { bail!("实例 Mods 已改变，请重新检查更新"); }
    if plan["blocked"].as_array().is_some_and(|b| !b.is_empty()) {bail!("兼容方案仍有未解决项目，不能应用");}
    let after: Lockfile=serde_json::from_value(plan["after"].clone())?;
    let mut target=i.clone();
    target.minecraft=after.environment.minecraft.clone();target.loader=after.environment.loader.clone();
    target.accepts(&after)?;
    let backup=snapshot(ws,i,"更新前")?;
    for id in plan["disabledIds"].as_array().into_iter().flatten() {
        let id=id.as_str().context("无效停用项")?;blocklink_model::validate_mod_id(id)?;
        let old=lock(ws,i)?.mods.into_iter().find(|m|m.mod_id==id).context("停用项已改变")?;
        anyhow::ensure!(!after.mods.iter().any(|m|m.mod_id==id),"停用方案不一致");
        write_json(&ws.instance_dir(&i.instance_id)?.join(format!("mod-disabled-{id}.json")),&old)?;
    }
    transition(ws,i,&target,&after)?;
    ws.verify_instance(&i.instance_id)?;
    Ok(json!({"snapshotId":backup,"updated":true}))
}

pub fn restore_snapshot(ws: &Workspace, i: &Instance, snapshot_id: &str) -> Result<Value> {
    blocklink_model::validate_uuid(snapshot_id)?;
    let value=read_json(&ws.instance_dir(&i.instance_id)?.join(format!("mod-snapshot-{snapshot_id}.json")))?;
    let target: Lockfile=serde_json::from_value(value["lock"].clone())?;
    let mut restored=i.clone();
    restored.minecraft=target.environment.minecraft.clone();restored.loader=target.environment.loader.clone();
    restored.accepts(&target)?;
    let backup=snapshot(ws,i,"回滚前")?;
    transition(ws,i,&restored,&target)?;
    ws.verify_instance(&i.instance_id)?;
    Ok(json!({"restored":true,"snapshotId":backup}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn update_rollback_toggle_and_stale_plan_preserve_data() -> Result<()> {
        use blocklink_model::{Runtime,JavaMode,Storage};
        let temp=tempfile::tempdir()?;
        let ws=Workspace::open(temp.path().join("data"))?;
        let i=Instance{schema_version:1,instance_id:uuid::Uuid::new_v4().to_string(),name:"Updates QA".into(),minecraft:"1.21.1".into(),loader:Loader::Fabric{version:"0.16.10".into()},runtime:Runtime{java:JavaMode::Auto,memory_mi_b:4096},storage:Storage{link_mode:LinkMode::Auto},mods:vec![],server:None};
        ws.create_instance(&i)?;
        let artifact=|version:&str| -> Result<Artifact> {
            let path=temp.path().join(format!("example-{version}.jar"));
            fs::write(&path,[b"PK\x03\x04".as_slice(),version.as_bytes()].concat())?;
            let blob=ws.import_jar(&path)?;
            Ok(Artifact{mod_id:"example".into(),version:version.into(),file:format!("example-{version}.jar"),sha512:blob.sha512,bytes:blob.bytes,side:Side::Both,source:Source::Local,dependencies:vec![]})
        };
        let mut old=lock(&ws,&i)?;old.mods.push(artifact("1.0")?);apply(&ws,&i,&old)?;
        let mut target=old.clone();target.mods[0]=artifact("2.0")?;
        target.mods[0].mod_id="renamed-example".into();
        let plan=ws.instance_dir(&i.instance_id)?.join("mod-update-plan.json");
        write_json(&plan,&json!({"id":"plan","before":old,"after":target}))?;
        let world=ws.instance_dir(&i.instance_id)?.join("game/world");fs::create_dir_all(&world)?;fs::write(world.join("level.dat"),b"world data")?;
        write_json(&plan,&json!({"id":"blocked","before":old,"after":target,"blocked":["unknown local Mod"]}))?;
        assert!(apply_updates(&ws,&i,"blocked").is_err());
        assert_eq!(lock(&ws,&i)?.mods[0].version,"1.0");
        write_json(&plan,&json!({"id":"plan","before":old,"after":target}))?;
        let result=apply_updates(&ws,&i,"plan")?;
        assert_eq!(lock(&ws,&i)?.mods[0].version,"2.0");
        assert_eq!(lock(&ws,&i)?.mods[0].mod_id,"renamed-example");
        assert!(!ws.instance_dir(&i.instance_id)?.join("game/mods/example-1.0.jar").exists());
        assert!(apply_updates(&ws,&i,"plan").is_err());
        restore_snapshot(&ws,&i,field(&result,"snapshotId")?)?;
        assert_eq!(lock(&ws,&i)?.mods[0].version,"1.0");
        assert_eq!(lock(&ws,&i)?.mods[0].mod_id,"example");
        assert_eq!(fs::read(world.join("level.dat"))?,b"world data");
        toggle(&ws,&i,"example",false)?;assert!(lock(&ws,&i)?.mods.is_empty());assert_eq!(disabled(&ws,&i)?.len(),1);
        toggle(&ws,&i,"example",true)?;assert_eq!(lock(&ws,&i)?.mods[0].version,"1.0");assert!(disabled(&ws,&i)?.is_empty());
        let mut with_dep=lock(&ws,&i)?;let mut consumer=artifact("3.0")?;consumer.mod_id="consumer".into();consumer.dependencies.push(Dependency{mod_id:"example".into(),version:"1.0".into()});with_dep.mods.push(consumer);apply(&ws,&i,&with_dep)?;
        assert!(toggle(&ws,&i,"example",false).is_err());assert_eq!(lock(&ws,&i)?.mods.len(),2);
        assert!(restore_snapshot(&ws,&i,"../outside").is_err());
        ws.verify_instance(&i.instance_id)?;
        Ok(())
    }
    #[test]
    fn compatibility_selects_older_release_and_preserves_matching_current() -> Result<()> {
        let i:Instance=serde_json::from_value(json!({"schemaVersion":1,"instanceId":uuid::Uuid::new_v4().to_string(),"name":"QA","minecraft":"1.21.4","loader":{"kind":"fabric","version":"0.16.10"},"runtime":{"java":"auto","memoryMiB":4096},"storage":{"linkMode":"auto"},"mods":[]}))?;
        let current=json!({"id":"new","game_versions":["26.2"],"loaders":["fabric"],"version_type":"release","date_published":"2026-09"});
        let older=json!({"id":"old","game_versions":["1.21.4"],"loaders":["fabric"],"version_type":"release","date_published":"2025-01"});
        let beta=json!({"id":"beta","game_versions":["1.21.4"],"loaders":["fabric"],"version_type":"beta","date_published":"2026-09"});
        assert_eq!(compatible_target(&current,&json!([current,older,beta]),&i).unwrap()["id"],"old");
        assert_eq!(compatible_target(&older,&json!([]),&i).unwrap()["id"],"old");
        assert!(compatible_target(&current,&json!([beta]),&i).is_none());
        assert!(loader_matches(&json!(">=0.16.9 <0.17.0"),"0.16.10")?);
        assert!(!loader_matches(&json!(">=0.16.14"),"0.16.10")?);
        assert!(!loader_matches(&json!("0.16.9"),"0.16.10")?);
        assert!(loader_matches(&json!([">=0.18.0", ">=0.16.0"]),"0.16.10")?);
        assert!(loader_matches(&json!("not-a-range"),"0.16.10").is_err());
        assert_eq!(parse_mod_json("{\"description\":\"line1\nline2\",\"id\":\"example\"}")?["id"],"example");
        assert!(parse_mod_json("{invalid}").is_err());
        Ok(())
    }
    #[test]
    fn reads_neoforge_and_rejects_other_loader_jars() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("example.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
        zip.start_file(
            "META-INF/neoforge.mods.toml",
            zip::write::SimpleFileOptions::default(),
        )?;
        zip.write_all(b"modLoader = 'javafml'\nloaderVersion = '[4,)'\nlicense = 'MIT'\n[[mods]]\nmodId = 'example'\nversion = '${file.jarVersion}'\n")?;
        zip.start_file(
            "META-INF/MANIFEST.MF",
            zip::write::SimpleFileOptions::default(),
        )?;
        zip.write_all(b"Manifest-Version: 1.0\r\nImplementation-Version: 2.3.4\r\n")?;
        zip.finish()?;
        let neo = Loader::NeoForge {
            version: "21.1.250".into(),
        };
        let (id, version, _) = inspect(&path, &neo)?;
        assert_eq!(id, "example");
        assert_eq!(version, "2.3.4");
        assert!(inspect(
            &path,
            &Loader::Fabric {
                version: "0.19.5".into()
            }
        )
        .is_err());
        let other = temp.path().join("forge.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&other)?);
        zip.start_file(
            "META-INF/mods.toml",
            zip::write::SimpleFileOptions::default(),
        )?;
        zip.write_all(b"[[mods]]\nmodId='forge_only'\nversion='1.0'\n[[dependencies.forge_only]]\nmodId='forge'\nversionRange='[47,)'\n")?;
        zip.finish()?;
        assert!(inspect(&other, &neo).is_err());
        assert_eq!(
            inspect(
                &other,
                &Loader::Forge {
                    version: "65.1.3".into()
                }
            )?
            .0,
            "forge_only"
        );
        assert!(inspect(
            &path,
            &Loader::Forge {
                version: "65.1.3".into()
            }
        )
        .is_err());
        let quilt_path = temp.path().join("quilt.jar");
        let mut zip = zip::ZipWriter::new(fs::File::create(&quilt_path)?);
        zip.start_file("quilt.mod.json", zip::write::SimpleFileOptions::default())?;
        zip.write_all(br#"{"quilt_loader":{"id":"quilt_example","version":"1.0"},"minecraft":{"environment":"client"}}"#)?;
        zip.finish()?;
        let parsed = inspect(
            &quilt_path,
            &Loader::Quilt {
                version: "0.30.1".into(),
            },
        )?;
        assert_eq!(parsed.0, "quilt_example");
        assert_eq!(parsed.2, Side::Client);
        assert!(inspect(
            &quilt_path,
            &Loader::Fabric {
                version: "0.19.5".into()
            }
        )
        .is_err());
        Ok(())
    }
}

// Only repair unescaped control characters inside JSON strings; never rewrite JAR bytes.
pub(crate) fn parse_mod_json(text: &str) -> Result<Value> {
    let mut out=String::with_capacity(text.len());
    let (mut quoted,mut escaped)=(false,false);
    for c in text.chars() {
        if quoted && !escaped && c.is_control() {
            out.push_str(&format!("\\u{:04x}",c as u32));
            continue;
        }
        out.push(c);
        if escaped {escaped=false;} else if quoted && c=='\\' {escaped=true;} else if c=='"' {quoted=!quoted;}
    }
    Ok(serde_json::from_str(&out)?)
}
pub(crate) fn loader_matches(requirement: &Value, version: &str) -> Result<bool> {
    if let Some(a)=requirement.as_array() {
        for r in a {if loader_matches(r,version)? {return Ok(true);}}
        return Ok(false);
    }
    let r=requirement.as_str().context("无法识别 Loader 版本要求")?;
    let version=semver::Version::parse(version)?;
    // Fabric uses spaces between conjunctive comparisons; semver uses commas.
    let r=r.split_whitespace().map(|p| if p.bytes().all(|b| b.is_ascii_digit() || b==b'.') {format!("={p}")} else {p.to_owned()}).collect::<Vec<_>>().join(", ");
    Ok(semver::VersionReq::parse(&r).context("无法自动判断此 Loader 版本要求")?.matches(&version))
}
fn check_fabric_loader(path: &Path, loader: &Loader) -> Result<()> {
    let Loader::Fabric{version}=loader else {return Ok(());};
    let mut zip=zip::ZipArchive::new(fs::File::open(path)?)?;
    let mut text=String::new();
    zip.by_name("fabric.mod.json")?.take(1_048_577).read_to_string(&mut text)?;
    anyhow::ensure!(text.len()<=1_048_576,"Mod 描述过大");
    let meta=parse_mod_json(&text)?;
    if let Some(r)=meta["depends"].get("fabricloader") {
        anyhow::ensure!(loader_matches(r,version)?,"{} 需要 Fabric Loader {}，当前为 {}",meta["id"],r,version);
    }
    Ok(())
}
fn supports(v: &Value, i: &Instance) -> bool {
    v["game_versions"].as_array().is_some_and(|a|a.contains(&json!(i.minecraft))) &&
    v["loaders"].as_array().is_some_and(|a|a.contains(&json!(i.loader.kind())))
}
fn compatible_target(current: &Value, candidates: &Value, i: &Instance) -> Option<Value> {
    if supports(current,i) {return Some(current.clone());}
    candidates.as_array()?.iter().filter(|v|v["version_type"]=="release" && supports(v,i))
        .max_by_key(|v|v["date_published"].as_str().unwrap_or("")).cloned()
}

pub fn prepare_compatibility(ws: &Workspace,i: &Instance,server: bool,allow_disable: bool,decisions: &Value,report: Reporter)->Result<Value> {
    anyhow::ensure!(!matches!(i.loader,Loader::Vanilla),"原版实例没有可适配的 Mods");
    let before=lock(ws,i)?;
    let mut target_instance=i.clone();
    if let Loader::Fabric{version}= &i.loader {
        let available=crate::game::loaders(&i.minecraft)?;
        let candidate=available.as_array().context("Loader 列表无效")?.iter()
            .filter(|v|v["loader"]["stable"]==true)
            .filter_map(|v|v["loader"]["version"].as_str().and_then(|s|semver::Version::parse(s).ok().map(|v|(v,s))))
            .max_by(|a,b|a.0.cmp(&b.0));
        if let Some((latest,text))=candidate {if latest>semver::Version::parse(version)? {target_instance.loader=Loader::Fabric{version:text.into()};}}
    }
    let original_instance=i;
    let i=&target_instance;
    if !before.mods.is_empty() {ws.verify_instance(&i.instance_id)?;}
    let mut identified=serde_json::Map::new();
    for chunk in before.mods.chunks(100) {
        report("识别本地 Mod 文件来源".into());
        let found:Value=client()?.post("https://api.modrinth.com/v2/version_files")
            .json(&json!({"hashes":chunk.iter().map(|a|&a.sha512).collect::<Vec<_>>(),"algorithm":"sha512"}))
            .send()?.error_for_status()?.json()?;
        identified.extend(found.as_object().context("Mod 来源查询响应无效")?.clone());
    }
    let mut rows=Vec::new();
    let mut blocked=Vec::new();
    let mut targets=Vec::new();
    let mut disabled_ids=Vec::new();
    let mut pins=HashSet::new();
    for a in &before.mods {
        let decision=decisions[&a.mod_id].as_str().unwrap_or("auto");
        anyhow::ensure!(["auto","keep","disable"].contains(&decision),"无效逐项选择");
        if decision=="disable" {disabled_ids.push(a.mod_id.clone());rows.push(json!({"modId":a.mod_id,"from":a.version,"status":"disable","reason":"手动选择停用"}));continue;}
        report(format!("匹配 {} · {} / {}",a.mod_id,i.minecraft,i.loader.kind()));
        let mut may_disable=false;
        let result=(|| -> Result<Value> {
            may_disable=!identified.contains_key(&a.sha512);
            let current=identified.get(&a.sha512).context("Modrinth 未收录此文件，无法可靠识别替代版本")?;
            anyhow::ensure!(current["files"].as_array().is_some_and(|files|files.iter().any(|f|f["hashes"]["sha512"]==a.sha512)),"来源文件哈希不匹配");
            let candidates=if supports(current,i) {json!([])}else{project_versions(field(current,"project_id")?,&i.minecraft,i.loader.kind())?};
            let selected=if decision=="keep" {pins.insert(a.mod_id.clone());if supports(current,i){Some(current.clone())}else{None}}else{compatible_target(current,&candidates,i)};
            may_disable=selected.is_none();
            let selected=selected.context("没有支持当前 Minecraft 和 Loader 的正式版本")?;
            anyhow::ensure!(selected["project_id"]==current["project_id"],"候选版本不属于已识别的 Mod 项目");
            targets.push((a.mod_id.clone(),field(&selected,"project_id")?.to_owned(),field(&selected,"id")?.to_owned()));
            Ok(json!({"modId":a.mod_id,"from":a.version,"to":selected["version_number"],"status":if selected["id"]==current["id"] {"compatible"}else{"replace"}}))
        })();
        match result {
            Ok(row)=>rows.push(row),
            Err(e)=>{let reason=format!("{e:#}");let disable=allow_disable && may_disable && decision!="keep";
                if disable {disabled_ids.push(a.mod_id.clone());}else{blocked.push(format!("{}：{}",a.mod_id,reason));}
                rows.push(json!({"modId":a.mod_id,"from":a.version,"status":if disable {"disable"}else{"blocked"},"canDisable":may_disable,"reason":reason}));}
        }
    }
    let mut after=before.clone();
    let mut changes=Vec::new();
    // Map verified project roots, not guessed names. Dependencies retain the IDs
    // read from their actual target JARs and are validated as a complete lockfile.
    let mut replacements:HashMap<String,String>=HashMap::new();
    if blocked.is_empty() {
        let resolved=(|| -> Result<Lockfile> {
            let mut r=Resolver{ws,i,server,report:report.clone(),visiting:HashSet::new(),resolved:HashMap::new(),projects:HashMap::new(),stable_only:true,incompatible:Vec::new(),choices:HashMap::new()};
            report("检查依赖组合，冲突时尝试较旧候选版本".into());
            r.choices=solver::solve(i,&targets,&pins)?;
            for (old_id,project,version) in targets {
                let version=r.choices.get(&project).cloned().unwrap_or(version);
                let a=r.resolve(&project,Some(&version))?;
                anyhow::ensure!(!replacements.contains_key(&a.mod_id),"多个现有 Mod 映射到同一目标 ID：{}",a.mod_id);
                replacements.insert(a.mod_id.clone(),old_id.clone());
                if a.mod_id!=old_id {
                    if let Some(row)=rows.iter_mut().find(|row|row["modId"]==old_id) {
                        row["targetModId"]=json!(a.mod_id);
                        row["reason"]=json!(format!("同一 Modrinth 项目，内部 ID 自动适配：{old_id} → {}",a.mod_id));
                    }
                }
            }
            for (owner,project,version) in &r.incompatible {
                anyhow::ensure!(!(if version.is_empty(){r.projects.contains_key(project)}else{r.resolved.contains_key(version)}),"{} 与 {} {} 声明不兼容",owner,project,version);
            }
            let mut target=before.clone();target.environment.loader=i.loader.clone();target.environment.minecraft=i.minecraft.clone();target.mods=r.resolved.into_values().collect();target.mods.sort_by(|a,b|a.mod_id.cmp(&b.mod_id));
            anyhow::ensure!(!target.mods.iter().any(|m|disabled_ids.contains(&m.mod_id)),"待停用 Mod 仍被其他项目作为依赖引入，请手动处理");
            i.accepts(&target)?;
            let check=crate::preflight::inspect_target(ws,i,&target)?;
            anyhow::ensure!(check["ready"]==true,"候选环境运行检查未通过：{}",check["errors"]);
            Ok(target)
        })();
        match resolved {Ok(target)=>after=target,Err(e)=>blocked.push(format!("依赖检查未通过：{e:#}"))}
    }
    if blocked.is_empty() {
        changes=after.mods.iter().filter_map(|a|{
            let old_id=replacements.get(&a.mod_id).unwrap_or(&a.mod_id);
            let old=before.mods.iter().find(|m|&m.mod_id==old_id);
            if old.is_some_and(|m|m.sha512==a.sha512){return None;}
            Some(json!({"modId":a.mod_id,"previousModId":old_id,"from":old.map(|m|&m.version),"to":a.version,"dependency":old.is_none(),"bytes":a.bytes}))
        }).collect();
    }
    if blocked.is_empty() {for row in &mut rows {if row["status"]!="disable" {if let Some(a)=after.mods.iter().find(|a|replacements.get(&a.mod_id).is_some_and(|old|row["modId"]==*old)){row["to"]=json!(a.version);row["status"]=json!(if before.mods.iter().any(|old|old.mod_id==a.mod_id && old.sha512==a.sha512){"compatible"}else{"replace"});}}}}
    if blocked.is_empty() {for id in &disabled_ids {let old=before.mods.iter().find(|m| &m.mod_id==id).unwrap();changes.push(json!({"modId":id,"from":old.version,"to":"停用（原文件保留）","dependency":false,"bytes":0}));}}
    let plan=json!({"id":uuid::Uuid::new_v4().to_string(),"kind":"compatibility","decisions":decisions,"loaderBefore":original_instance.loader,"loaderAfter":i.loader,"createdAt":crate::auth::now(),"before":before,"after":after,"changes":changes,"rows":rows,"blocked":blocked,"disabledIds":disabled_ids,"skipped":[],"ready":blocked.is_empty()});
    write_json(&ws.instance_dir(&i.instance_id)?.join("mod-update-plan.json"),&plan)?;
    Ok(plan)
}

pub(super) fn transition(ws:&Workspace, current:&Instance, target:&Instance, after:&Lockfile)->Result<()> {
    anyhow::ensure!(current.instance_id==target.instance_id,"实例身份不一致");
    target.accepts(after)?;
    for m in &after.mods {ws.verify_blob(&m.sha512,m.bytes)?;}
    let dir=ws.instance_dir(&current.instance_id)?;
    let journal=dir.join("environment-transition.json");
    anyhow::ensure!(!journal.exists(),"存在未完成的环境切换，请重启后台恢复");
    let before=lock(ws,current)?;
    write_json(&journal,&json!({"instance":current,"lock":before}))?;
    let result=(||->Result<()> {
        write_json(&dir.join("instance.json"),target)?;
        apply(ws,target,after)?;
        ws.verify_instance(&target.instance_id)?;
        fs::remove_file(&journal)?;
        Ok(())
    })();
    if let Err(e)=result {
        ws.recover_all()?;
        write_json(&dir.join("instance.json"),current)?;
        apply(ws,current,&before).context("环境切换失败且回滚未完成，请重启后台恢复")?;
        fs::remove_file(journal)?;
        return Err(e);
    }
    Ok(())
}

pub fn recover_environments(ws:&Workspace)->Result<()> {
    for i in ws.instances()? {
        let dir=ws.instance_dir(&i.instance_id)?;
        let p=dir.join("environment-transition.json");
        if p.exists() {
            let v=read_json(&p)?;
            let old:Instance=serde_json::from_value(v["instance"].clone())?;
            let old_lock:Lockfile=serde_json::from_value(v["lock"].clone())?;
            anyhow::ensure!(old.instance_id==i.instance_id,"恢复记录实例身份不符");
            old.accepts(&old_lock)?;
            write_json(&dir.join("instance.json"),&old)?;
            apply(ws,&old,&old_lock)?;
            fs::remove_file(p)?;
        }
    }
    Ok(())
}
