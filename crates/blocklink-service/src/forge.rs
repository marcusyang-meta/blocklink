use crate::{game, installer, net::*};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::path::Path;

fn parse_versions(xml: &str, mc: &str) -> Result<Vec<String>> {
    let prefix = format!("{mc}-");
    let re = regex::Regex::new(r"<version>([^<]+)</version>")?;
    let mut versions: Vec<_> = re
        .captures_iter(xml)
        .filter_map(|c| c[1].strip_prefix(&prefix).map(str::to_owned))
        .filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
        .collect();
    versions.sort_by_cached_key(|v| {
        v.split('.')
            .map(|p| p.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    });
    versions.dedup();
    versions.reverse();
    Ok(versions)
}
pub fn versions(mc: &str) -> Result<Value> {
    blocklink_model::validate_version(mc, true)?;
    let xml = client()?
        .get("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
        .send()?
        .error_for_status()?
        .text()?;
    ensure!(xml.len() < 4_000_000, "Forge 版本清单过大");
    Ok(json!(parse_versions(&xml, mc)?
        .into_iter()
        .map(|v| json!({"loader":{"version":v,"stable":true}}))
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
    let coordinate = format!("{mc}-{version}");
    installer::install(installer::Distribution {
        name: "Forge", coordinate: format!("net/minecraftforge/forge/{coordinate}"),
        url: format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{coordinate}/forge-{coordinate}-installer.jar"),
    },root,id,mc,java,server,report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_game_and_numeric_order() {
        let xml="<version>26.2-65.1.3</version><version>26.2-65.1.12</version><version>26.1.2-64.0.1</version><version>26.2-65.0.1-beta</version>";
        assert_eq!(
            parse_versions(xml, "26.2").unwrap(),
            vec!["65.1.12", "65.1.3"]
        );
        assert!(parse_versions(xml, "26.1").unwrap().is_empty());
    }
}
