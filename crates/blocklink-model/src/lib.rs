//! Portable, side-effect-free configuration and resolved-environment contracts.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(pub String);
pub type Result<T> = std::result::Result<T, ValidationError>;
fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ValidationError(message.into()))
    }
}

pub fn validate_uuid(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).map_err(|_| ValidationError("Invalid UUID".into()))?;
    require(parsed.to_string() == id, "UUID must be canonical lowercase")
}
pub fn validate_hash(hash: &str) -> Result<()> {
    require(
        hash.len() == 128
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Expected lowercase SHA-512",
    )
}
pub fn validate_mod_id(id: &str) -> Result<()> {
    require(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
            && id.as_bytes()[0].is_ascii_alphanumeric(),
        "Invalid logical Mod ID",
    )
}
pub fn validate_version(version: &str, exact: bool) -> Result<()> {
    require(
        !version.is_empty()
            && version.len() <= 128
            && version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
            && version.as_bytes()[0].is_ascii_alphanumeric(),
        "Invalid version",
    )?;
    require(
        !exact || !["latest", "recommended"].contains(&version),
        "Lockfile requires an exact version",
    )
}
pub fn filename_key(name: &str) -> String {
    name.nfkc().flat_map(char::to_lowercase).collect()
}
pub fn validate_filename(name: &str) -> Result<()> {
    require(
        name.chars().count() <= 180 && name.ends_with(".jar") && name.len() > 4,
        "Expected a portable .jar filename",
    )?;
    require(
        !name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)),
        "Filename contains a forbidden character",
    )?;
    let normalized: String = name.nfkc().collect();
    require(
        !normalized
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)),
        "Normalized filename is unsafe",
    )?;
    require(
        !name.starts_with('.') && !name.ends_with([' ', '.']),
        "Ambiguous filename",
    )?;
    let stem = normalized
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    require(!reserved, "Windows reserved filename")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Loader {
    Vanilla,
    Fabric { version: String },
    NeoForge { version: String },
    Forge { version: String },
    Quilt { version: String },
}
impl Loader {
    pub fn validate(&self, exact: bool) -> Result<()> {
        match self {
            Self::Vanilla => Ok(()),
            Self::Fabric { version }
            | Self::NeoForge { version }
            | Self::Forge { version }
            | Self::Quilt { version } => validate_version(version, exact),
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Fabric { .. } => "fabric",
            Self::NeoForge { .. } => "neoforge",
            Self::Forge { .. } => "forge",
            Self::Quilt { .. } => "quilt",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub minecraft: String,
    pub loader: Loader,
}
impl Environment {
    pub fn validate(&self) -> Result<()> {
        validate_version(&self.minecraft, true)?;
        self.loader.validate(true)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    Both,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    #[default]
    Auto,
    Hardlink,
    Copy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Source {
    Local,
    Https {
        url: String,
    },
    Modrinth {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "versionId")]
        version_id: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dependency {
    pub mod_id: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub mod_id: String,
    pub version: String,
    pub file: String,
    pub sha512: String,
    pub bytes: u64,
    pub side: Side,
    pub source: Source,
    pub dependencies: Vec<Dependency>,
}
impl Artifact {
    pub fn validate(&self) -> Result<()> {
        validate_mod_id(&self.mod_id)?;
        validate_version(&self.version, true)?;
        validate_filename(&self.file)?;
        validate_hash(&self.sha512)?;
        require(
            self.bytes > 0 && self.bytes <= 1_073_741_824,
            "Invalid artifact size",
        )?;
        match &self.source {
            Source::Https { url } => require(
                url.starts_with("https://")
                    && url.len() > 8
                    && url.len() <= 4096
                    && !url.chars().any(char::is_whitespace),
                "Expected HTTPS source",
            )?,
            Source::Modrinth {
                project_id,
                version_id,
            } => {
                require(
                    !project_id.is_empty()
                        && project_id.len() <= 128
                        && project_id
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
                    "Invalid Modrinth project ID",
                )?;
                require(
                    !version_id.is_empty()
                        && version_id.len() <= 64
                        && version_id.bytes().all(|c| c.is_ascii_alphanumeric()),
                    "Invalid Modrinth version ID",
                )?;
            }
            Source::Local => {}
        }
        require(self.dependencies.len() <= 4096, "Too many dependencies")?;
        for d in &self.dependencies {
            validate_mod_id(&d.mod_id)?;
            validate_version(&d.version, true)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lockfile {
    pub schema_version: u32,
    pub environment: Environment,
    pub mods: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ContentBundle>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContentBundle {pub sha512: String, pub bytes: u64}
impl Lockfile {
    pub fn validate(&self) -> Result<()> {
        require(
            self.schema_version == 1,
            "Unsupported lockfile schemaVersion",
        )?;
        self.environment.validate()?;
        if let Some(content)=&self.content {validate_hash(&content.sha512)?;require(content.bytes>0&&content.bytes<=268_435_456,"Invalid content bundle size")?;}
        require(self.mods.len() <= 4096, "Too many Mods")?;
        require(
            !matches!(self.environment.loader, Loader::Vanilla) || self.mods.is_empty(),
            "Vanilla cannot load Mods",
        )?;
        let mut names = HashSet::new();
        let mut ids = HashMap::new();
        for m in &self.mods {
            m.validate()?;
            require(
                names.insert(filename_key(&m.file)),
                format!("Filename collision: {}", m.file),
            )?;
            require(
                ids.insert(&m.mod_id, &m.version).is_none(),
                format!("Duplicate Mod ID: {}", m.mod_id),
            )?;
        }
        for m in &self.mods {
            for d in &m.dependencies {
                require(
                    ids.get(&d.mod_id).is_some_and(|v| **v == d.version),
                    format!(
                        "Unresolved dependency {}@{} required by {}",
                        d.mod_id, d.version, m.mod_id
                    ),
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Runtime {
    pub java: JavaMode,
    pub memory_mi_b: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JavaMode {
    Auto,
    Manual,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Storage {
    pub link_mode: LinkMode,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryRequest {
    pub mod_id: String,
    pub project_id: String,
    pub version: String,
    pub side: Side,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalRequest {
    pub mod_id: String,
    pub sha512: String,
    pub side: Side,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModRequest {
    Registry(RegistryRequest),
    Local(LocalRequest),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerBinding {
    pub server_id: String,
    pub endpoint: String,
    pub sync_policy: SyncPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncPolicy {
    Ask,
    Automatic,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Instance {
    pub schema_version: u32,
    pub instance_id: String,
    pub name: String,
    pub minecraft: String,
    pub loader: Loader,
    pub runtime: Runtime,
    pub storage: Storage,
    pub mods: Vec<ModRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerBinding>,
}
impl Instance {
    pub fn validate(&self) -> Result<()> {
        require(
            self.schema_version == 1,
            "Unsupported instance schemaVersion",
        )?;
        validate_uuid(&self.instance_id)?;
        require(
            !self.name.trim().is_empty() && self.name.chars().count() <= 80,
            "Invalid instance name",
        )?;
        validate_version(&self.minecraft, true)?;
        self.loader.validate(false)?;
        require(
            (512..=131072).contains(&self.runtime.memory_mi_b),
            "Invalid memoryMiB",
        )?;
        require(self.mods.len() <= 4096, "Too many Mod requests")?;
        let mut ids = HashSet::new();
        for request in &self.mods {
            let id = match request {
                ModRequest::Local(m) => {
                    validate_hash(&m.sha512)?;
                    &m.mod_id
                }
                ModRequest::Registry(m) => {
                    require(
                        !m.project_id.is_empty()
                            && m.project_id.len() <= 128
                            && m.project_id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
                        "Invalid project ID",
                    )?;
                    require(
                        !m.version.is_empty() && m.version.len() <= 128,
                        "Invalid requested version",
                    )?;
                    &m.mod_id
                }
            };
            validate_mod_id(id)?;
            require(ids.insert(id), "Duplicate requested Mod ID")?;
        }
        if let Some(s) = &self.server {
            validate_uuid(&s.server_id)?;
            require(
                s.endpoint.starts_with("https://")
                    && s.endpoint.len() > 8
                    && s.endpoint.len() <= 4096
                    && !s.endpoint.chars().any(char::is_whitespace),
                "Expected HTTPS server endpoint",
            )?;
        }
        Ok(())
    }
    pub fn accepts(&self, lock: &Lockfile) -> Result<()> {
        self.validate()?;
        lock.validate()?;
        require(
            self.minecraft == lock.environment.minecraft,
            "Minecraft version mismatch",
        )?;
        match (&self.loader, &lock.environment.loader) {
            (Loader::Vanilla, Loader::Vanilla) => Ok(()),
            (Loader::Fabric { version: requested }, Loader::Fabric { version: resolved }) => {
                require(
                    requested == "recommended" || requested == resolved,
                    "Fabric version mismatch",
                )
            }
            (Loader::NeoForge { version: requested }, Loader::NeoForge { version: resolved })
            | (Loader::Forge { version: requested }, Loader::Forge { version: resolved })
            | (Loader::Quilt { version: requested }, Loader::Quilt { version: resolved }) => {
                require(
                    requested == "recommended" || requested == resolved,
                    "Loader version mismatch",
                )
            }
            _ => Err(ValidationError("Loader mismatch".into())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublishTarget {
    Running,
    NextStart,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientLockRef {
    pub sha512: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerManifest {
    pub schema_version: u32,
    pub server_id: String,
    pub revision: u64,
    pub published_at: String,
    pub target: PublishTarget,
    pub environment: Environment,
    pub client_lock: ClientLockRef,
}
impl ServerManifest {
    pub fn validate(&self) -> Result<()> {
        require(
            self.schema_version == 1,
            "Unsupported manifest schemaVersion",
        )?;
        validate_uuid(&self.server_id)?;
        require(
            (1..=9_007_199_254_740_991).contains(&self.revision),
            "Invalid revision",
        )?;
        time::OffsetDateTime::parse(
            &self.published_at,
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|_| ValidationError("Invalid publishedAt".into()))?;
        self.environment.validate()?;
        validate_hash(&self.client_lock.sha512)?;
        require(
            (2..=16_777_216).contains(&self.client_lock.bytes),
            "Invalid lockfile size",
        )
    }
}
