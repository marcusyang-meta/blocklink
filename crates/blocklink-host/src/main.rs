use blocklink_core::{read_instance, read_lockfile, Workspace};
use blocklink_model::{
    Artifact, Environment, Instance, JavaMode, LinkMode, Loader, Lockfile, Runtime, Side, Source,
    Storage,
};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "blocklink",
    version,
    about = "Blocklink native core — local Mod store and recoverable instance transactions"
)]
struct Cli {
    /// Explicit data root. This milestone never modifies another launcher's data.
    #[arg(long)]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create an independent instance, or import a validated instance.json.
    Create {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, required_unless_present = "config")]
        name: Option<String>,
        #[arg(long, default_value = "1.21.1")]
        minecraft: String,
        #[arg(long, value_enum, default_value = "vanilla")]
        loader: LoaderArg,
        #[arg(long)]
        loader_version: Option<String>,
    },
    List,
    /// Import a JAR into the content store without changing any instance.
    Import {
        file: PathBuf,
    },
    /// Validate the portable lockfile without writing to the data directory.
    Validate {
        lock: PathBuf,
    },
    /// Apply a fully resolved lockfile using objects already in the store.
    Apply {
        instance: String,
        lock: PathBuf,
        #[arg(long, value_enum, default_value = "auto")]
        mode: ModeArg,
        #[cfg(debug_assertions)]
        #[arg(long, hide = true)]
        test_crash_at: Option<String>,
        #[cfg(debug_assertions)]
        #[arg(long, hide = true)]
        test_pause_at: Option<String>,
    },
    /// Import a local JAR and update the instance's resolved lockfile by Mod ID.
    Add {
        instance: String,
        file: PathBuf,
        #[arg(long)]
        mod_id: String,
        #[arg(long)]
        version: String,
        #[arg(long, value_enum, default_value = "both")]
        side: SideArg,
        #[arg(long, value_enum, default_value = "auto")]
        mode: ModeArg,
    },
    /// Remove a Mod from this instance; shared store content is retained.
    Remove {
        instance: String,
        mod_id: String,
    },
    /// Hash-check applied Mods, their shared objects, and the installed receipt.
    Verify {
        instance: String,
    },
    /// Recover interrupted commits or remove incomplete preparation directories.
    Recover,
}
#[derive(Clone, ValueEnum)]
enum LoaderArg {
    Vanilla,
    Fabric,
    Neoforge,
    Forge,
    Quilt,
}
#[derive(Clone, ValueEnum)]
enum ModeArg {
    Auto,
    Hardlink,
    Copy,
}
#[derive(Clone, ValueEnum)]
enum SideArg {
    Client,
    Server,
    Both,
}
impl From<ModeArg> for LinkMode {
    fn from(v: ModeArg) -> Self {
        match v {
            ModeArg::Auto => Self::Auto,
            ModeArg::Hardlink => Self::Hardlink,
            ModeArg::Copy => Self::Copy,
        }
    }
}
impl From<SideArg> for Side {
    fn from(v: SideArg) -> Self {
        match v {
            SideArg::Client => Self::Client,
            SideArg::Server => Self::Server,
            SideArg::Both => Self::Both,
        }
    }
}
fn print(value: impl serde::Serialize) -> Result<(), blocklink_core::Error> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn run(cli: Cli) -> blocklink_core::Result<()> {
    if let Command::Validate { lock } = &cli.command {
        let lock = read_lockfile(lock)?;
        return print(serde_json::json!({"valid":true,"mods":lock.mods.len()}));
    }
    let workspace = Workspace::open(&cli.root)?;
    // All write/read commands first recover pending transactions. Recovery reports
    // go to stderr; stdout remains machine-readable for desktop/CLI integrations.
    let recovered = workspace.recover_all()?;
    if !recovered.is_empty() {
        eprintln!(
            "{}",
            serde_json::to_string(&serde_json::json!({"recovered":recovered}))?
        );
    }
    match cli.command {
        Command::Create {
            config,
            name,
            minecraft,
            loader,
            loader_version,
        } => {
            let instance = if let Some(config) = config {
                read_instance(config)?
            } else {
                Instance{schema_version:1,instance_id:uuid::Uuid::new_v4().to_string(),name:name.unwrap(),minecraft,loader:match loader{LoaderArg::Forge=>Loader::Forge{version:loader_version.clone().ok_or_else(||blocklink_core::Error::Invalid("--loader-version is required".into()))?},LoaderArg::Quilt=>Loader::Quilt{version:loader_version.clone().ok_or_else(||blocklink_core::Error::Invalid("--loader-version is required".into()))?},LoaderArg::Neoforge=>Loader::NeoForge{version:loader_version.ok_or_else(||blocklink_core::Error::Invalid("--loader-version is required".into()))?},LoaderArg::Vanilla=>Loader::Vanilla,LoaderArg::Fabric=>Loader::Fabric{version:loader_version.ok_or_else(||blocklink_core::Error::Invalid("--loader-version is required for Fabric until the installer is connected".into()))?}},runtime:Runtime{java:JavaMode::Auto,memory_mi_b:4096},storage:Storage{link_mode:LinkMode::Auto},mods:vec![],server:None}
            };
            workspace.create_instance(&instance)?;
            print(instance)
        }
        Command::List => print(workspace.instances()?),
        Command::Import { file } => print(workspace.import_jar(file)?),
        Command::Apply {
            instance,
            lock,
            mode,
            #[cfg(debug_assertions)]
            test_crash_at,
            #[cfg(debug_assertions)]
            test_pause_at,
        } => {
            let lock = read_lockfile(lock)?;
            let receipt =
                workspace.sync_with_checkpoints(&instance, &lock, mode.into(), |point| {
                    #[cfg(debug_assertions)]
                    if test_crash_at.as_deref() == Some(point.as_str()) {
                        std::process::exit(79);
                    }
                    #[cfg(debug_assertions)]
                    if test_pause_at.as_deref() == Some(point.as_str()) {
                        eprintln!("TEST_CHECKPOINT:{}", point.as_str());
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                    let _ = point;
                })?;
            print(receipt)
        }
        Command::Add {
            instance,
            file,
            mod_id,
            version,
            side,
            mode,
        } => {
            let intent = workspace.instance(&instance)?;
            let mut lock = workspace.read_lock(&instance)?.unwrap_or(Lockfile {
                content: None,
                schema_version: 1,
                environment: Environment {
                    minecraft: intent.minecraft.clone(),
                    loader: intent.loader.clone(),
                },
                mods: vec![],
            });
            let filename = file
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| {
                    blocklink_core::Error::Invalid("Expected UTF-8 JAR filename".into())
                })?
                .to_owned();
            // Validate metadata before importing. This milestone does not infer Mod
            // identity or solve dependencies from JAR metadata.
            blocklink_model::validate_mod_id(&mod_id)?;
            blocklink_model::validate_version(&version, true)?;
            blocklink_model::validate_filename(&filename)?;
            let blob = workspace.import_jar(file)?;
            lock.mods.retain(|m| m.mod_id != mod_id);
            lock.mods.push(Artifact {
                mod_id,
                version,
                file: filename,
                sha512: blob.sha512,
                bytes: blob.bytes,
                side: side.into(),
                source: Source::Local,
                dependencies: vec![],
            });
            print(workspace.sync(&instance, &lock, mode.into())?)
        }
        Command::Remove { instance, mod_id } => {
            let mut lock = workspace
                .read_lock(&instance)?
                .ok_or_else(|| blocklink_core::Error::Invalid("No applied lockfile".into()))?;
            let before = lock.mods.len();
            lock.mods.retain(|m| m.mod_id != mod_id);
            if before == lock.mods.len() {
                return Err(blocklink_core::Error::Invalid(
                    "Mod ID not installed".into(),
                ));
            }
            print(workspace.sync(&instance, &lock, LinkMode::Auto)?)
        }
        Command::Verify { instance } => print(workspace.verify_instance(&instance)?),
        Command::Recover => print(serde_json::json!({"recovered":recovered})),
        Command::Validate { .. } => unreachable!(),
    }
}
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "{}",
                serde_json::json!({"error":{"code":e.code(),"message":e.to_string()}})
            );
            ExitCode::from(1)
        }
    }
}
