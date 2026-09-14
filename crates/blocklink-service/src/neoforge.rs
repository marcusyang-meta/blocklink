use crate::{game, net::*};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::path::Path;
fn matches_mc(version: &str, mc: &str) -> bool {
    let p: Vec<_> = mc.split('.').collect();
    if p.first() == Some(&"1") && p.len() >= 2 {
        // Modern NeoForge starts at Minecraft 1.20.2; 1.20.1 used different coordinates.
        if p[1] == "20" && p.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0) < 2 {
            return false;
        }
        version.starts_with(&format!("{}.{}.", p[1], p.get(2).unwrap_or(&"0")))
    } else if p.len() >= 2 {
        version.starts_with(&format!("{}.{}.{}.", p[0], p[1], p.get(2).unwrap_or(&"0")))
    } else {
        false
    }
}
pub fn versions(mc: &str) -> Result<Value> {
    blocklink_model::validate_version(mc, true)?;
    let xml = client()?
        .get("https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml")
        .send()?
        .error_for_status()?
        .text()?;
    ensure!(xml.len() < 2_000_000, "NeoForge 版本清单过大");
    let re = regex::Regex::new(r"<version>([^<]+)</version>")?;
    let mut found: Vec<_> = re
        .captures_iter(&xml)
        .map(|c| c[1].to_owned())
        .filter(|v| matches_mc(v, mc))
        .collect();
    found.sort_by_cached_key(|v| {
        (
            v.split('-')
                .next()
                .unwrap_or(v)
                .split('.')
                .map(|n| n.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>(),
            !v.contains('-'),
        )
    });
    found.reverse();
    Ok(json!(found
        .into_iter()
        .map(|v| json!({"loader":{"stable":!v.contains('-'),"version":v}}))
        .collect::<Vec<_>>()))
}
pub fn install(
    root: &Path,
    id: &str,
    mc: &str,
    version: &str,
    java: &Path,
    server: bool,
    report: &game::Reporter,
) -> Result<Value> {
    blocklink_model::validate_version(version, true)?;
    ensure!(
        matches_mc(version, mc),
        "NeoForge {version} 不适用于 Minecraft {mc}"
    );
    crate::installer::install(crate::installer::Distribution {
        name: "NeoForge", coordinate: format!("net/neoforged/neoforge/{version}"),
        url: format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"),
    },root,id,mc,java,server,report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_mapping() {
        assert!(matches_mc("21.1.250", "1.21.1"));
        assert!(!matches_mc("21.10.2", "1.21.1"));
        assert!(matches_mc("26.2.0.87", "26.2"));
        assert!(matches_mc("26.1.2.109", "26.1.2"));
        assert!(!matches_mc("20.1.1", "1.20.1"));
        assert!(matches_mc("20.2.88", "1.20.2"));
    }
}
