use super::AppState;
use serde_json::{json,Value};
use tauri_plugin_updater::UpdaterExt;
use std::{path::PathBuf,sync::Mutex,time::Duration};
pub struct Pending(pub Mutex<Option<tauri_plugin_updater::Update>>);
#[tauri::command]
pub async fn check_update(app:tauri::AppHandle,pending:tauri::State<'_,Pending>)->Result<Value,String>{
    let update=app.updater_builder().timeout(Duration::from_secs(30)).build().map_err(|e|e.to_string())?.check().await.map_err(|e|e.to_string())?;
    let result=update.as_ref().map(|u|json!({"version":u.version,"currentVersion":u.current_version,"notes":u.body})).unwrap_or(Value::Null);
    *pending.0.lock().unwrap()=update;Ok(result)
}
#[tauri::command]
pub async fn install_update(app:tauri::AppHandle,state:tauri::State<'_,AppState>,pending:tauri::State<'_,Pending>,progress:tauri::ipc::Channel<Value>)->Result<(),String>{
    #[cfg(target_os="linux")]
    if std::env::var_os("APPIMAGE").is_none(){return Err("Automatic replacement requires the portable AppImage. Update .deb installations with your package manager.".into())}
    let update=pending.0.lock().unwrap().take().ok_or("Check for an update first")?;
    let mut received=0u64;
    let bytes=update.download(|n,total|{received+=n as u64;let _=progress.send(json!({"received":received,"total":total}));},||{}).await.map_err(|e|e.to_string())?;
    let root=state.0.clone();
    // Verify the signed download before asking the service to stop. Never stop a running game.
    #[cfg(windows)]
    let helper={
        use std::io::Write;
        let target=std::env::current_exe().map_err(|e|e.to_string())?;
        let parent=target.parent().ok_or("Invalid app directory")?;
        let helper=parent.join(format!(".blocklink-update-{}.exe",uuid::Uuid::new_v4()));
        let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&helper).map_err(|e|format!("Cannot write the launcher directory: {e}"))?;
        file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
        (helper,target)
    };
    let service_root=root.clone();
    let ready=tauri::async_runtime::spawn_blocking(move||blocklink_service::rpc(&service_root,"prepare-app-update",json!({}))).await.map_err(|e|e.to_string())?;
    if let Err(e)=ready{
        #[cfg(windows)] let _=std::fs::remove_file(&helper.0);
        return Err(e.to_string())
    }
    #[cfg(windows)]{
        use std::os::windows::process::CommandExt;
        if let Err(error)=std::process::Command::new(&helper.0).arg("--apply-launcher-update").arg(&helper.1).arg(&root).creation_flags(0x08000000).spawn(){return Err(format!("Unable to restart: {error}. Reopen Blocklink to reconnect."))}
        app.exit(0);Ok(())
    }
    #[cfg(not(windows))]{
        // AppImage and macOS .app replacement is handled by the official updater.
        if let Err(error)=update.install(bytes){return Err(format!("{error}. Reopen Blocklink to reconnect."))}
        app.restart();
    }
}

// Runs from the verified new executable after the UI exits. Keep a backup until replacement succeeds.
#[cfg(windows)]
pub fn apply_windows(target:PathBuf,root:PathBuf)->Result<(),String>{
    use std::{fs,process::Command,os::windows::process::CommandExt};
    let helper=std::env::current_exe().map_err(|e|e.to_string())?;
    if helper.parent()!=target.parent()||helper==target||target.extension().and_then(|s|s.to_str())!=Some("exe"){return Err("Invalid update destination".into())}
    let backup=target.with_file_name(format!(".blocklink-previous-{}.exe",uuid::Uuid::new_v4()));
    let mut replaced=false;
    for _ in 0..120 {
        if fs::rename(&target,&backup).is_ok(){replaced=true;break}
        std::thread::sleep(Duration::from_millis(500));
    }
    if !replaced{return Err("The launcher is still in use; close other Blocklink windows and retry".into())}
    if let Err(e)=fs::copy(&helper,&target){let _=fs::remove_file(&target);let _=fs::rename(&backup,&target);return Err(e.to_string())}
    if let Err(e)=Command::new(&target).arg("--data-dir").arg(&root).creation_flags(0x08000000).spawn(){let _=fs::remove_file(&target);let _=fs::rename(&backup,&target);let _=Command::new(&target).arg("--data-dir").arg(root).creation_flags(0x08000000).spawn();return Err(e.to_string())}
    // Keep the running helper in place; it cannot delete its own executable on Windows.
    let _=fs::remove_file(backup);Ok(())
}
