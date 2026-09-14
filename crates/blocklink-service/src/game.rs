use crate::net::*;
use anyhow::{bail, Context, Result};
use blocklink_model::{Instance, Loader};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

pub type Reporter = Arc<dyn Fn(String) + Send + Sync>;
pub fn os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}
pub fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x64"
    }
}
// Older macOS releases only ship Intel native libraries. Keep the JVM and
// native libraries on the same architecture instead of mixing arm64 and x64.
pub fn runtime_arch<'a>(meta: &Value, platform: &str, host_arch: &'a str, server: bool) -> &'a str {
    if platform == "osx" && host_arch == "aarch64" && meta["javaVersion"]["majorVersion"].as_u64().or(meta["major"].as_u64()).unwrap_or(8) < 11 { return "x64"; }
    if platform == "osx" && host_arch == "aarch64" && !server {
        let arm_natives = meta["libraries"].as_array().is_some_and(|libraries| {
            libraries.iter().any(|lib| {
                lib["name"].as_str().is_some_and(|name| name.contains(":natives-macos-arm64"))
                    || lib["downloads"]["classifiers"].as_object().is_some_and(|items| items.keys().any(|k| k.contains("macos-arm64")))
            })
        });
        if !arm_natives { return "x64"; }
    }
    host_arch
}
pub fn versions() -> Result<Value> {
    let all = json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
    Ok(Value::Array(
        all["versions"]
            .as_array()
            .context("版本列表无效")?
            .iter()
            .filter(|v| v["type"] == "release" && v["id"].as_str().is_some_and(supported_release))
            .cloned()
            .collect(),
    ))
}
fn supported_release(s: &str) -> bool {
    let parts: Vec<_> = s.split('.').map(str::parse::<u32>).collect();
    if parts.iter().any(Result::is_err) {
        return false;
    }
    let major = parts
        .first()
        .and_then(|v| v.as_ref().ok())
        .copied()
        .unwrap_or(0);
    let minor = parts
        .get(1)
        .and_then(|v| v.as_ref().ok())
        .copied()
        .unwrap_or(0);
    major > 1 || (major == 1 && minor >= 13)
}
pub fn loaders(version: &str) -> Result<Value> {
    blocklink_model::validate_version(version, true)?;
    json(&format!(
        "https://meta.fabricmc.net/v2/versions/loader/{version}"
    ))
}
#[cfg(test)]
fn allowed(rules: &Value) -> bool {
    allowed_on(rules, os(), arch())
}
fn allowed_on(rules: &Value, platform: &str, architecture: &str) -> bool {
    let Some(rules) = rules.as_array() else {
        return true;
    };
    let mut yes = false;
    for r in rules {
        if r["features"]
            .as_object()
            .is_some_and(|m| m.values().any(|v| v == true))
        {
            continue;
        }
        if let Some(name) = r["os"]["name"].as_str() {
            if name != platform {
                continue;
            }
        }
        if let Some(a) = r["os"]["arch"].as_str() {
            if !if architecture == "x64" {
                ["x86_64", "amd64"].contains(&a)
            } else {
                ["aarch64", "arm64"].contains(&a)
            } {
                continue;
            }
        }
        if let Some(p) = r["os"]["version"].as_str() {
            if !regex::Regex::new(p)
                .is_ok_and(|r| r.is_match(if platform == "windows" { "10.0" } else { "" }))
            {
                continue;
            }
        }
        yes = r["action"] == "allow";
    }
    yes
}
pub fn process_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(s) = path.to_str() {
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}
fn java_version(java: &Path) -> Result<u64> {
    let java = process_path(java);
    let out = hidden(Command::new(&java).arg("-version"))
        .output()
        .context("无法运行 Java")?;
    let text = String::from_utf8_lossy(&out.stderr);
    let re = regex::Regex::new(r#"version "(?:1\.)?(\d+)"#)?;
    let major = re
        .captures(&text)
        .and_then(|c| c.get(1))
        .context("无法读取 Java 版本")?
        .as_str()
        .parse()?;
    if !out.status.success() {
        bail!("Java 检查失败")
    };
    if major >= 9 {
        let modules = hidden(Command::new(&java).arg("--list-modules")).output()?;
        if !modules.status.success()
            || !String::from_utf8_lossy(&modules.stdout).contains("java.base@")
        {
            bail!("Java 模块加载检查失败，请重新安装运行环境")
        }
    }
    Ok(major)
}
fn find_java(dir: &Path) -> Option<PathBuf> {
    let bin = dir.join(if cfg!(windows) {
        "bin/java.exe"
    } else {
        "bin/java"
    });
    if bin.is_file() {
        return Some(bin);
    }
    fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .find_map(|e| find_java(&e.path()))
}
pub fn java(root: &Path, major: u64, manual: Option<&str>, report: &Reporter, architecture: &str) -> Result<PathBuf> {
    let root = process_path(root);
    let root = root.as_path();
    if os() == "osx" && arch() == "aarch64" && architecture == "x64" {
        let supported = Command::new("/usr/bin/arch").args(["-x86_64", "/usr/bin/true"]).status().is_ok_and(|status| status.success());
        if !supported { bail!("This Minecraft version needs Rosetta 2 on Apple Silicon. Install Rosetta using Apple's instructions, then retry."); }
    }
    if let Some(p) = manual.filter(|p| !p.trim().is_empty()) {
        let p = process_path(Path::new(p));
        if java_version(&p)? != major {
            bail!("此游戏需要 Java {major}")
        }
        let settings = hidden(Command::new(&p).args(["-XshowSettings:properties", "-version"])).output()?;
        let properties = format!("{}\n{}", String::from_utf8_lossy(&settings.stdout), String::from_utf8_lossy(&settings.stderr));
        let actual = properties.lines().filter_map(|line| line.trim().split_once('=')).find_map(|(key,value)| (key.trim()=="os.arch").then_some(value.trim()));
        let matches = match architecture {
            "x64" => matches!(actual, Some("amd64" | "x86_64")),
            "aarch64" => matches!(actual, Some("aarch64" | "arm64")),
            _ => false,
        };
        if !settings.status.success() || !matches { bail!("The selected Java runtime must use the {architecture} architecture. Clear the custom Java path to use automatic setup."); }
        return Ok(p);
    }
    let dest = root
        .join("runtime/java")
        .join(format!("{major}-{}-{architecture}", os()));
    if let Some(p) = find_java(&dest) {
        if java_version(&p).is_ok_and(|v| v == major) {
            return Ok(p);
        }
    }
    report(format!("自动下载 Java {major} · {} {architecture}", os()));
    let j_os = if os() == "osx" { "mac" } else { os() };
    let mut assets=json(&format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={architecture}&image_type=jre&os={j_os}&vendor=eclipse")).unwrap_or_else(|_|json!([]));
    if assets.as_array().is_none_or(|a| a.is_empty()) {
        assets=json(&format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={architecture}&image_type=jdk&os={j_os}&vendor=eclipse"))?;
    }
    if assets.as_array().is_none_or(|list| list.is_empty()) { bail!("Java {major} is not available for {j_os} {architecture}. Select a custom Java runtime in game settings."); }
    let package = &assets[0]["binary"]["package"];
    let name = field(package, "name")?;
    let archive = safe_join(&root.join("downloads"), name)?;
    download(
        field(package, "link")?,
        &archive,
        Some(("sha256", field(package, "checksum")?)),
    )?;
    let stage = tempfile::tempdir_in(root.join("downloads"))?;
    if name.ends_with(".zip") {
        extract_zip(&archive, stage.path())?
    } else {
        let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(&archive)?));
        let mut total = 0u64;
        for entry in ar.entries()? {
            let mut e = entry?;
            total += e.size();
            if total > 2_147_483_648 {
                bail!("Java 压缩包过大")
            }
            if (e.header().entry_type().is_file() || e.header().entry_type().is_dir())
                && !e.unpack_in(stage.path())?
            {
                bail!("Java 压缩路径越界")
            }
        }
    }
    let p = find_java(stage.path()).context("Java 包缺少可执行文件")?;
    if java_version(&p)? != major {
        bail!("Java 版本不匹配")
    }
    let rel = p.strip_prefix(stage.path())?.to_path_buf();
    fs::create_dir_all(dest.parent().unwrap())?;
    if dest.exists() {
        fs::rename(
            &dest,
            root.join("downloads")
                .join(uuid::Uuid::new_v4().to_string()),
        )?;
    }
    fs::rename(stage.path(), &dest)?;
    Ok(dest.join(rel))
}
pub fn install(
    root: &Path,
    instance: &Instance,
    server: bool,
    manual: Option<&str>,
    report: Reporter,
) -> Result<Value> {
    let root = process_path(root);
    let root = root.as_path();
    // Imported instances may pin a modern official snapshot absent from the release picker.
    let manifest = json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
    let entry = manifest["versions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == instance.minecraft && (
            (v["type"] == "release" && supported_release(&instance.minecraft)) ||
            (v["type"] == "snapshot" && instance.minecraft.starts_with("26."))))
        .cloned()
        .context("此版本不在支持的官方版本列表中（1.13+ 正式版或 26.x 快照）")?;
    let meta = json(field(&entry, "url")?)?;
    let major = meta["javaVersion"]["majorVersion"].as_u64().unwrap_or(8);
    let architecture = runtime_arch(&meta, os(), arch(), server);
    let java = java(root, major, manual, &report, architecture)?;
    let idir = root.join("instances").join(&instance.instance_id);
    let game = idir.join("game");
    fs::create_dir_all(&game)?;
    // A failed reinstall must never leave a stale success marker.
    if idir.join("installed.json").exists() {
        fs::remove_file(idir.join("installed.json"))?;
    }
    if server {
        if let Loader::NeoForge { version } | Loader::Forge { version } = &instance.loader {
            let install = if matches!(instance.loader, Loader::Forge { .. }) {
                crate::forge::install
            } else {
                crate::neoforge::install
            };
            let mut installed = install(
                root,
                &instance.instance_id,
                &instance.minecraft,
                version,
                &java,
                true,
                &report,
            )?;
            installed["java"] = json!(java);
            installed["major"] = json!(major);
            installed["minecraft"] = json!(instance.minecraft);
            installed["loader"] = json!(instance.loader);
            write_json(&idir.join("installed.json"), &installed)?;
            return Ok(installed);
        }
    }
    if server {
        let jar = idir.join("server.jar");
        report("安装服务器与 Loader".into());
        match &instance.loader {
            Loader::Vanilla => download(
                field(&meta["downloads"]["server"], "url")?,
                &jar,
                Some(("sha1", field(&meta["downloads"]["server"], "sha1")?)),
            )?,
            Loader::Fabric { version } => {
                let installers = json("https://meta.fabricmc.net/v2/versions/installer")?;
                let installer = field(&installers[0], "version")?;
                download(&format!("https://meta.fabricmc.net/v2/versions/loader/{}/{version}/{installer}/server/jar",instance.minecraft),&jar,None)?;
            }
            Loader::Quilt { version } => {
                let installed = crate::quilt::server(
                    root,
                    &instance.instance_id,
                    &instance.minecraft,
                    version,
                    &java,
                    &report,
                )?;
                let installed = json!({"java":java,"major":major,"serverJar":installed,"minecraft":instance.minecraft,"loader":instance.loader});
                write_json(&idir.join("installed.json"), &installed)?;
                return Ok(installed);
            }
            Loader::NeoForge { .. } | Loader::Forge { .. } => unreachable!(),
        }
        let installed = json!({"java":java,"major":major,"serverJar":jar,"minecraft":instance.minecraft,"loader":instance.loader});
        write_json(&idir.join("installed.json"), &installed)?;
        return Ok(installed);
    }
    report("安装 Minecraft 客户端".into());
    let jar = root
        .join("runtime/versions")
        .join(&instance.minecraft)
        .join(format!("{}.jar", instance.minecraft));
    download(
        field(&meta["downloads"]["client"], "url")?,
        &jar,
        Some(("sha1", field(&meta["downloads"]["client"], "sha1")?)),
    )?;
    write_json(
        &root
            .join("runtime/versions")
            .join(&instance.minecraft)
            .join(format!("{}.json", instance.minecraft)),
        &meta,
    )?;
    let extra = match &instance.loader {
        Loader::Vanilla => json!({}),
        Loader::Fabric { version } => json(&format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{version}/profile/json",
            instance.minecraft
        ))?,
        Loader::Quilt { version } => crate::quilt::profile(&instance.minecraft, version)?,
        Loader::Forge { version } => crate::forge::install(
            root,
            &instance.instance_id,
            &instance.minecraft,
            version,
            &java,
            false,
            &report,
        )?,
        Loader::NeoForge { version } => crate::neoforge::install(
            root,
            &instance.instance_id,
            &instance.minecraft,
            version,
            &java,
            false,
            &report,
        )?,
    };
    let mut libraries = extra["libraries"].as_array().cloned().unwrap_or_default();
    libraries.extend(meta["libraries"].as_array().cloned().unwrap_or_default());
    let mut cp = Vec::new();
    let mut seen = HashSet::new();
    let natives = idir.join("natives");
    fs::create_dir_all(&natives)?;
    for l in libraries {
        if !allowed_on(&l["rules"], os(), architecture) {
            continue;
        }
        let name = field(&l, "name")?;
        let parts: Vec<_> = name.split(':').collect();
        if parts.len() < 3 {
            bail!("无效依赖坐标")
        }
        if let Some(classifier) = parts.get(3) {
            if classifier.starts_with("natives-") {
                if classifier.ends_with("-x86") {
                    continue;
                }
                if architecture == "x64" && classifier.ends_with("-arm64") {
                    continue;
                }
                if architecture == "aarch64" && !classifier.ends_with("-arm64") {
                    continue;
                }
            }
        }
        let key = format!("{}:{}:{}", parts[0], parts[1], parts.get(3).unwrap_or(&""));
        // Older manifests split the classpath artifact and native classifiers
        // across duplicate library entries. Only deduplicate the classpath.
        let new_artifact = seen.insert(key);
        report(format!("安装游戏依赖 · {}", parts[1]));
        let mut artifact = l["downloads"]["artifact"].clone();
        if artifact.is_null() && l["downloads"].is_null() {
            let p = format!(
                "{}/{}/{}/{}-{}{}.jar",
                parts[0].replace('.', "/"),
                parts[1],
                parts[2],
                parts[1],
                parts[2],
                parts.get(3).map(|c| format!("-{c}")).unwrap_or_default()
            );
            artifact = json!({"path":p,"url":format!("{}{}",l["url"].as_str().unwrap_or("https://libraries.minecraft.net/"),p)})
        }
        if new_artifact && !artifact.is_null() {
            let dest = safe_join(&root.join("runtime/libraries"), field(&artifact, "path")?)?;
            download(
                field(&artifact, "url")?,
                &dest,
                artifact["sha1"].as_str().map(|s| ("sha1", s)),
            )?;
            cp.push(dest);
        }
        if let Some(native) = l["natives"][os()].as_str() {
            let n = &l["downloads"]["classifiers"][native.replace("${arch}", "64")];
            if !n.is_null() {
                let dest = safe_join(&root.join("runtime/libraries"), field(n, "path")?)?;
                download(
                    field(n, "url")?,
                    &dest,
                    n["sha1"].as_str().map(|s| ("sha1", s)),
                )?;
                extract_zip(&dest, &natives)?;
            }
        }
    }
    // Modern Mojang metadata includes arm64 native classifier artifacts on macOS.
    if cfg!(target_os = "macos") && architecture == "aarch64" {
        cp.retain(|p| !p.to_string_lossy().contains("natives-macos.jar"));
    }
    cp.push(jar);
    let index = &meta["assetIndex"];
    let index_path = safe_join(
        &root.join("runtime/assets/indexes"),
        &format!("{}.json", field(index, "id")?),
    )?;
    download(
        field(index, "url")?,
        &index_path,
        Some(("sha1", field(index, "sha1")?)),
    )?;
    let assets = read_json(&index_path)?;
    let objects: Vec<_> = assets["objects"]
        .as_object()
        .context("资源索引无效")?
        .values()
        .filter_map(|v| v["hash"].as_str().map(String::from))
        .collect();
    let cursor = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    std::thread::scope(|scope| -> Result<()> {
        let mut handles = vec![];
        for _ in 0..8 {
            let objects = &objects;
            let cursor = &cursor;
            let done = &done;
            let report = &report;
            handles.push(scope.spawn(move || -> Result<()> {
                loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    if i >= objects.len() {
                        break;
                    }
                    let hash = &objects[i];
                    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                        bail!("资源哈希无效")
                    };
                    let p = format!("{}/{}", &hash[..2], hash);
                    download(
                        &format!("https://resources.download.minecraft.net/{p}"),
                        &root.join("runtime/assets/objects").join(&p),
                        Some(("sha1", hash)),
                    )?;
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(50) || n == objects.len() {
                        report(format!("安装资源 · {n} / {}", objects.len()))
                    }
                }
                Ok(())
            }));
        }
        for h in handles {
            h.join().map_err(|_| anyhow::anyhow!("下载任务中断"))??;
        }
        Ok(())
    })?;
    if !meta["logging"]["client"].is_null() {
        let l = &meta["logging"]["client"]["file"];
        download(
            field(l, "url")?,
            &safe_join(&root.join("runtime/logging"), field(l, "id")?)?,
            Some(("sha1", field(l, "sha1")?)),
        )?;
    }
    let installed = json!({"java":java,"major":major,"meta":meta,"extra":extra,"cp":cp,"natives":natives,"minecraft":instance.minecraft,"loader":instance.loader});
    write_json(&idir.join("installed.json"), &installed)?;
    report("安装完成".into());
    Ok(installed)
}
fn expand(list: &Value, vars: &HashMap<String, String>) -> Result<Vec<String>> {
    let mut out = vec![];
    let re = regex::Regex::new(r"\$\{([^}]+)\}")?;
    for a in list.as_array().into_iter().flatten() {
        let values = if let Some(s) = a.as_str() {
            vec![s]
        } else if allowed_on(&a["rules"], os(), vars.get("blocklink_runtime_arch").map(String::as_str).unwrap_or(arch())) {
            if let Some(s) = a["value"].as_str() {
                vec![s]
            } else {
                a["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect()
            }
        } else {
            vec![]
        };
        for s in values {
            for c in re.captures_iter(s) {
                if !vars.contains_key(&c[1]) {
                    bail!("不支持启动参数 {}", &c[1])
                }
            }
            out.push(
                re.replace_all(s, |c: &regex::Captures| vars[&c[1]].clone())
                    .into_owned(),
            )
        }
    }
    Ok(out)
}
pub fn launch_args(
    root: &Path,
    i: &Instance,
    installed: &Value,
    account: &Value,
) -> Result<Vec<String>> {
    let root = process_path(root);
    let root = root.as_path();
    let meta = &installed["meta"];
    let extra = &installed["extra"];
    let cp = installed["cp"]
        .as_array()
        .context("游戏尚未安装")?
        .iter()
        .filter_map(Value::as_str)
        .map(|p| process_path(Path::new(p)).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(if cfg!(windows) { ";" } else { ":" });
    let pairs = [
        ("blocklink_runtime_arch", runtime_arch(meta, os(), arch(), false).to_owned()),
        ("auth_player_name", field(account, "name")?.to_owned()),
        ("auth_uuid", field(account, "id")?.to_owned()),
        ("auth_access_token", field(account, "token")?.to_owned()),
        (
            "version_name",
            // NeoForge's ignoreList uses this basename to exclude the unpatched
            // vanilla jar from its transforming module layer.
            if matches!(i.loader, Loader::NeoForge { .. } | Loader::Forge { .. }) {
                i.minecraft.clone()
            } else {
                extra["id"].as_str().unwrap_or(&i.minecraft).into()
            },
        ),
        (
            "game_directory",
            root.join("instances")
                .join(&i.instance_id)
                .join("game")
                .to_string_lossy()
                .into(),
        ),
        (
            "assets_root",
            root.join("runtime/assets").to_string_lossy().into(),
        ),
        (
            "assets_index_name",
            field(&meta["assetIndex"], "id")?.into(),
        ),
        (
            "natives_directory",
            process_path(Path::new(field(installed, "natives")?))
                .to_string_lossy()
                .into_owned(),
        ),
        ("launcher_name", "Blocklink".into()),
        ("launcher_version", "0.1.0".into()),
        ("classpath", cp),
        (
            "classpath_separator",
            if cfg!(windows) { ";" } else { ":" }.into(),
        ),
        (
            "library_directory",
            root.join("runtime/libraries").to_string_lossy().into(),
        ),
        (
            "user_type",
            if account["offline"] == true {
                "legacy"
            } else {
                "msa"
            }
            .into(),
        ),
        ("version_type", "release".into()),
        ("user_properties", "{}".into()),
        ("auth_xuid", account["xuid"].as_str().unwrap_or("").into()),
        (
            "clientid",
            account["clientId"].as_str().unwrap_or("").into(),
        ),
    ];
    let vars = pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    let mut args = vec![format!("-Xmx{}M", i.runtime.memory_mi_b)];
    args.extend(expand(&meta["arguments"]["jvm"], &vars)?);
    args.extend(expand(&extra["arguments"]["jvm"], &vars)?);
    if let Some(a) = meta["logging"]["client"]["argument"].as_str() {
        args.push(
            a.replace(
                "${path}",
                &safe_join(
                    &root.join("runtime/logging"),
                    field(&meta["logging"]["client"]["file"], "id")?,
                )?
                .to_string_lossy(),
            ),
        );
    }
    args.push(
        extra["mainClass"]
            .as_str()
            .unwrap_or(field(meta, "mainClass")?)
            .into(),
    );
    args.extend(expand(&meta["arguments"]["game"], &vars)?);
    args.extend(expand(&extra["arguments"]["game"], &vars)?);
    Ok(args)
}
pub fn multiplayer_args(mc: &str, port: u16) -> Vec<String> {
    let modern = mc.split('.').next() == Some("1")
        && mc
            .split('.')
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .is_some_and(|v| v < 20);
    if modern {
        vec![
            "--server".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
        ]
    } else {
        vec!["--quickPlayMultiplayer".into(), format!("127.0.0.1:{port}")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn platform_and_native_architecture_stay_aligned() {
        let modern = json!({"javaVersion":{"majorVersion":21},"libraries":[{"name":"org.lwjgl:lwjgl:3.3.3:natives-macos-arm64"}]});
        let old = json!({"javaVersion":{"majorVersion":17},"libraries":[]});
        assert_eq!(runtime_arch(&modern,"osx","aarch64",false),"aarch64");
        assert_eq!(runtime_arch(&old,"osx","aarch64",false),"x64");
        assert_eq!(runtime_arch(&old,"osx","aarch64",true),"aarch64");
        assert_eq!(runtime_arch(&json!({}),"osx","aarch64",true),"x64");
        for (platform, architecture) in [("windows","x64"),("linux","x64"),("osx","x64")] {
            assert_eq!(runtime_arch(&modern,platform,architecture,false),architecture);
            assert!(allowed_on(&json!([{"action":"allow","os":{"name":platform,"arch":"x86_64"}}]),platform,architecture));
        }
        assert!(!allowed_on(&json!([{"action":"allow","os":{"name":"windows"}}]),"linux","x64"));
        assert!(!allowed_on(&json!([{"action":"allow","os":{"name":"osx","arch":"aarch64"}}]),"osx","x64"));
    }
    #[test]
    #[ignore = "requires BLOCKLINK_TEST_JAVA pointing to a real managed Java runtime"]
    fn managed_java_loads_module_image() {
        let java = std::env::var("BLOCKLINK_TEST_JAVA").unwrap();
        let version = java_version(Path::new(&java)).unwrap();
        assert!([17, 21].contains(&version));
        let temp = tempfile::tempdir().unwrap();
        let report: Reporter = Arc::new(|_| {});
        assert!(super::java(temp.path(),version,Some(&java),&report,arch()).is_ok());
        let wrong = if arch()=="x64" { "aarch64" } else { "x64" };
        assert!(super::java(temp.path(),version,Some(&java),&report,wrong).is_err());
    }
    #[test]
    fn rules_and_arguments() {
        for v in ["1.13", "1.16.5", "1.18.2", "1.21.11", "26.2"] {
            assert!(supported_release(v));
        }
        for v in ["1.12.2", "26.3-rc-2", "../bad"] {
            assert!(!supported_release(v));
        }
        assert_eq!(multiplayer_args("1.16.5", 25565)[0], "--server");
        assert_eq!(multiplayer_args("26.2", 25565)[0], "--quickPlayMultiplayer");
        assert!(allowed(&Value::Null));
        assert!(!allowed(
            &json!([{"action":"allow","features":{"is_demo_user":true}}])
        ));
        let vars = HashMap::from([("a".into(), "有 空格".into())]);
        assert_eq!(
            expand(
                &json!(["${a}",{"rules":[{"action":"allow"}],"value":["x","y"]}]),
                &vars
            )
            .unwrap(),
            vec!["有 空格", "x", "y"]
        );
        assert!(expand(&json!(["${missing}"]), &vars).is_err());
    }
}
