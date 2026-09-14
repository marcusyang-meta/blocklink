use crate::{game, net::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn jar_json(path: &Path, name: &str) -> Result<Value> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path)?)?;
    let mut b = Vec::new();
    zip.by_name(name)?.take(4_194_305).read_to_end(&mut b)?;
    ensure!(b.len() <= 4_194_304, "安装描述过大");
    Ok(serde_json::from_slice(&b)?)
}
pub struct Distribution {
    pub name: &'static str,
    pub coordinate: String,
    pub url: String,
}
pub fn install(
    spec: Distribution,
    root: &Path,
    id: &str,
    mc: &str,
    java: &Path,
    server: bool,
    report: &game::Reporter,
) -> Result<Value> {
    let url = &spec.url;
    let name = spec.name;
    let hash = client()?
        .get(format!("{url}.sha1"))
        .send()?
        .error_for_status()?
        .text()?;
    let hash = hash.split_whitespace().next().context("安装器校验值为空")?;
    ensure!(
        hash.len() == 40 && hash.bytes().all(|c| c.is_ascii_hexdigit()),
        "无效安装器校验值"
    );
    let jar = root.join("downloads").join(format!(
        "{}-installer.jar",
        spec.coordinate.replace('/', "-")
    ));
    download(url, &jar, Some(("sha1", hash)))?;
    let profile = jar_json(&jar, "install_profile.json")?;
    ensure!(profile["minecraft"] == mc, "安装器游戏版本不匹配");
    let extra = jar_json(&jar, "version.json")?;
    ensure!(extra["inheritsFrom"] == mc, "客户端继承版本不匹配");
    let target = if server {
        root.join("instances").join(id).join("game")
    } else {
        root.join("runtime")
    };
    fs::create_dir_all(&target)?;
    if !server && !target.join("launcher_profiles.json").exists() {
        write_json(
            &target.join("launcher_profiles.json"),
            &json!({"profiles":{}}),
        )?;
    }
    let log_path = root.join("instances").join(id).join("installer.log");
    let log = fs::File::create(&log_path)?;
    report(format!("安装 {name} · 下载依赖并生成补丁"));
    let mut child = hidden(
        Command::new(game::process_path(java))
            .arg("-jar")
            .arg(game::process_path(&jar))
            .arg(if server {
                "--installServer"
            } else {
                "--installClient"
            })
            .arg(game::process_path(&target))
            .current_dir(&target)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log),
    )
    .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut tick = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "{name} 安装失败；详细日志：{}",
                log_path.display()
            );
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("{name} 安装超时，详细日志：{}", log_path.display());
        }
        if tick.elapsed() > Duration::from_secs(5) {
            report(format!("{name} 官方安装器正在处理依赖与补丁，请稍候"));
            tick = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if server {
        let args = target.join(format!(
            "libraries/{}/{}_args.txt",
            spec.coordinate,
            if cfg!(windows) { "win" } else { "unix" }
        ));
        if !args.is_file() && name == "Forge" {
            let version = spec
                .coordinate
                .rsplit('/')
                .next()
                .context("缺少 Forge 坐标")?;
            let jar = target.join(format!("forge-{version}.jar"));
            ensure!(jar.is_file(), "安装器未生成服务器启动文件");
            return Ok(json!({"serverJar":jar}));
        }
        ensure!(args.is_file(), "安装器未生成服务器启动参数");
        Ok(json!({"serverArgsFile":args}))
    } else {
        let installed = read_json(&safe_join(
            &target.join("versions"),
            &format!("{0}/{0}.json", field(&extra, "id")?),
        )?)?;
        ensure!(installed["inheritsFrom"] == mc, "生成的客户端配置错误");
        Ok(installed)
    }
}
