use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;
use sha2::Digest;
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::Duration,
};

pub fn client() -> Result<Client> {
    static CLIENT: std::sync::OnceLock<Client> = std::sync::OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c.clone());
    }
    let c = Client::builder()
        .user_agent("Blocklink/0.1.0 (Minecraft desktop launcher)")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(240))
        .https_only(true)
        .build()?;
    let _ = CLIENT.set(c.clone());
    Ok(c)
}
pub fn json(url: &str) -> Result<Value> {
    Ok(client()?.get(url).send()?.error_for_status()?.json()?)
}
pub fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().with_context(|| format!("响应缺少 {key}"))
}
pub fn safe_join(root: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if name.contains('\\')
        || name.contains(':')
        || path.is_absolute()
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        bail!("不安全的文件路径");
    }
    Ok(root.join(path))
}
pub fn hash_file(path: &Path, kind: &str) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut b = [0u8; 65536];
    let mut a = sha1::Sha1::new();
    let mut c = sha2::Sha256::new();
    let mut d = sha2::Sha512::new();
    loop {
        let n = f.read(&mut b)?;
        if n == 0 {
            break;
        }
        match kind {
            "sha1" => a.update(&b[..n]),
            "sha256" => c.update(&b[..n]),
            "sha512" => d.update(&b[..n]),
            _ => bail!("未知校验算法"),
        };
    }
    Ok(match kind {
        "sha1" => format!("{:x}", a.finalize()),
        "sha256" => format!("{:x}", c.finalize()),
        _ => format!("{:x}", d.finalize()),
    })
}
pub fn download(url: &str, dest: &Path, checksum: Option<(&str, &str)>) -> Result<()> {
    crate::transfers::check()?;
    if dest.is_file() {
        if let Some((k, h)) = checksum {
            if hash_file(dest, k)? == h {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }
    fs::create_dir_all(dest.parent().context("无效下载路径")?)?;
    let mut temp = tempfile::NamedTempFile::new_in(dest.parent().unwrap())?;
    let mut response = client()?.get(url).send()?.error_for_status()?;
    let total=response.content_length();
    let count = crate::transfers::copy(&mut response, &mut temp,total,&dest.file_name().unwrap_or_default().to_string_lossy())?;
    if count > 2_147_483_648 {
        bail!("下载超过大小限制")
    }
    temp.flush()?;
    if let Some((k, h)) = checksum {
        if hash_file(temp.path(), k)? != h {
            bail!("下载校验失败：{url}")
        }
    }
    temp.persist(dest).map_err(|e| e.error)?;
    Ok(())
}
pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    fs::create_dir_all(path.parent().context("无效路径")?)?;
    let mut f = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer_pretty(&mut f, value)?;
    f.flush()?;
    f.as_file().sync_all()?;
    f.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
    let mut total = 0;
    for i in 0..zip.len() {
        let mut e = zip.by_index(i)?;
        total += e.size();
        if total > 4_294_967_296u64 {
            bail!("压缩包超过大小限制")
        }
        if e.name().starts_with("META-INF/") {
            continue;
        }
        let rel = e.enclosed_name().context("压缩包路径越界")?;
        if e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            bail!("压缩包包含符号链接")
        }
        let p = dest.join(rel);
        if e.is_dir() {
            fs::create_dir_all(p)?
        } else {
            fs::create_dir_all(p.parent().unwrap())?;
            let mut f = fs::File::create(&p)?;
            std::io::copy(&mut e, &mut f)?;
        }
    }
    Ok(())
}
pub fn hidden(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confined_paths() {
        let r = Path::new("root");
        for x in ["../x", "/x", "C:/x", "a\\..\\x"] {
            assert!(safe_join(r, x).is_err())
        }
        assert!(safe_join(r, "a/b.jar").is_ok());
    }
}
