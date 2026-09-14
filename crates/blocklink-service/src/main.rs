use clap::Parser;
use std::path::PathBuf;
#[derive(Parser)]
#[command(version, about = "Blocklink headless host and local administration")]
struct Cli {
    /// Data directory. Without --request, runs the persistent host.
    data_dir: Option<PathBuf>,
    #[arg(long)]
    root: Option<PathBuf>,
    /// Send an action to an already running local host (status, create, install, launch, stop...).
    #[arg(long)]
    request: Option<String>,
    /// UTF-8 JSON object containing request fields; defaults to {}.
    #[arg(long)]
    payload_file: Option<PathBuf>,
    /// Wait for a queued job and exit nonzero on failure.
    #[arg(long)]
    wait: bool,
}
fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = cli
        .root
        .or(cli.data_dir)
        .unwrap_or_else(blocklink_service::default_root);
    if let Some(action) = cli.request {
        let payload = if let Some(file) = cli.payload_file {
            serde_json::from_slice(&std::fs::read(file)?)?
        } else {
            serde_json::json!({})
        };
        let result = blocklink_service::rpc(&root, &action, payload)?;
        if cli.wait {
            if let Some(id) = result["jobId"].as_str() {
                loop {
                    let state = blocklink_service::rpc(&root, "status", serde_json::json!({}))?;
                    let job = state["jobs"]
                        .as_array()
                        .and_then(|j| j.iter().find(|j| j["id"] == id))
                        .ok_or_else(|| anyhow::anyhow!("任务已丢失，后台可能重新启动"))?;
                    match job["status"].as_str() {
                        Some("done") => {
                            println!("{}", serde_json::to_string_pretty(&job["result"])?);
                            return Ok(());
                        }
                        Some("error") => {
                            anyhow::bail!("{}", job["message"].as_str().unwrap_or("任务失败"))
                        }
                        _ => std::thread::sleep(std::time::Duration::from_millis(500)),
                    }
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&result)?);
        Ok(())
    } else {
        blocklink_service::serve(root)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1)
    }
}
