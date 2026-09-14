use crate::{game, net::*};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn supported_pair(mc: &str, loader: &str) -> bool {
    // The per-game API also returns historical loaders; use our modern baseline.
    if mc
        .split('.')
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|v| v >= 26)
    {
        let base = loader
            .split('-')
            .next()
            .unwrap_or(loader)
            .split('.')
            .filter_map(|v| v.parse::<u32>().ok())
            .collect::<Vec<_>>();
        base.as_slice() >= [0, 30, 1].as_slice()
    } else {
        true
    }
}
pub fn versions(mc: &str) -> Result<Value> {
    blocklink_model::validate_version(mc, true)?;
    let games = json("https://meta.quiltmc.org/v3/versions/game")?;
    if !games
        .as_array()
        .context("无效 Quilt 游戏清单")?
        .iter()
        .any(|v| v["version"] == mc)
    {
        return Ok(json!([]));
    }
    let mut loaders = json(&format!("https://meta.quiltmc.org/v3/versions/loader/{mc}"))?
        .as_array()
        .context("无效 Quilt 版本清单")?
        .clone();
    loaders.retain(|v| {
        v["loader"]["version"]
            .as_str()
            .is_some_and(|l| supported_pair(mc, l))
    });
    loaders.sort_by_cached_key(|v| {
        let s = v["loader"]["version"].as_str().unwrap_or("");
        (
            s.split('-')
                .next()
                .unwrap_or(s)
                .split('.')
                .map(|p| p.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>(),
            !s.contains('-'),
            s.rsplit('.')
                .next()
                .and_then(|p| p.parse::<u32>().ok())
                .unwrap_or(0),
        )
    });
    loaders.reverse();
    for v in &mut loaders {
        v["loader"]["stable"] = json!(!field(&v["loader"], "version")?.contains('-'));
    }
    Ok(json!(loaders))
}
pub fn profile(mc: &str, version: &str) -> Result<Value> {
    blocklink_model::validate_version(mc, true)?;
    blocklink_model::validate_version(version, true)?;
    ensure!(
        supported_pair(mc, version),
        "26.x 请使用 Quilt 0.30.1 或更新版本"
    );
    let p = json(&format!(
        "https://meta.quiltmc.org/v3/versions/loader/{mc}/{version}/profile/json"
    ))?;
    ensure!(p["inheritsFrom"] == mc, "Quilt 配置游戏版本不匹配");
    Ok(p)
}
pub fn server(
    root: &Path,
    id: &str,
    mc: &str,
    version: &str,
    java: &Path,
    report: &game::Reporter,
) -> Result<PathBuf> {
    // Validate the exact pair before executing an installer.
    profile(mc, version)?;
    let installers = json("https://meta.quiltmc.org/v3/versions/installer")?;
    let i = installers
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|v| v["version"].as_str().is_some_and(|v| !v.contains('-')))
        })
        .context("没有可用的 Quilt 安装器")?;
    let iv = field(i, "version")?;
    blocklink_model::validate_version(iv, true)?;
    let jar = root
        .join("downloads")
        .join(format!("quilt-installer-{iv}.jar"));
    // The meta API can retain an older build's hash. Verify against the
    // checksum shipped alongside the artifact in Quilt's official Maven.
    let url=format!("https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/{iv}/quilt-installer-{iv}.jar");
    let checksum = client()?
        .get(format!("{url}.sha256"))
        .send()?
        .error_for_status()?
        .text()?;
    let checksum = checksum
        .split_whitespace()
        .next()
        .context("Quilt 安装器校验值为空")?;
    ensure!(
        checksum.len() == 64 && checksum.bytes().all(|c| c.is_ascii_hexdigit()),
        "Quilt 安装器校验值无效"
    );
    download(&url, &jar, Some(("sha256", checksum)))?;
    let target = root.join("instances").join(id).join("game");
    fs::create_dir_all(&target)?;
    let log_path = root.join("instances").join(id).join("installer.log");
    let log = fs::File::create(&log_path)?;
    report(format!("安装 Quilt {version} 服务器"));
    let mut child = hidden(
        Command::new(game::process_path(java))
            .arg("-jar")
            .arg(game::process_path(&jar))
            .args(["install", "server", mc, version, "--download-server"])
            .arg(format!(
                "--install-dir={}",
                game::process_path(&target).display()
            ))
            .current_dir(&target)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log),
    )
    .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut tick = Instant::now();
    loop {
        if let Some(s) = child.try_wait()? {
            ensure!(s.success(), "Quilt 安装失败，详见 {}", log_path.display());
            break;
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("Quilt 安装超时，详见 {}", log_path.display());
        }
        if tick.elapsed() > Duration::from_secs(5) {
            report("Quilt 官方安装器正在下载服务器依赖".into());
            tick = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let launcher = target.join("quilt-server-launch.jar");
    ensure!(
        launcher.is_file(),
        "Quilt 安装器未生成服务器入口，详见 {}",
        log_path.display()
    );
    Ok(launcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_game_excludes_historical_loader_entries() {
        assert!(!supported_pair("26.2", "0.20.0-beta.9"));
        assert!(supported_pair("26.2", "0.30.1"));
        assert!(supported_pair("26.2", "0.31.0-beta.4"));
        assert!(supported_pair("1.20.1", "0.20.0"));
    }
}
