#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde_json::Value;
use std::path::PathBuf;
struct AppState(PathBuf);
mod updates;
#[tauri::command]
async fn pick_world(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|f| f.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn pick_mod(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Minecraft Mod", &["jar"])
            .blocking_pick_file()
            .map(|f| f.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn pick_pack(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Minecraft Modpack", &["mrpack", "zip"])
            .blocking_pick_file()
            .map(|f| f.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn save_pack(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || app.dialog().file().add_filter("Minecraft Modpack", &["mrpack"]).set_file_name("My-modpack.mrpack").blocking_save_file().map(|f|f.to_string())).await.map_err(|e|e.to_string())
}
#[tauri::command]
fn open_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if ![
        "https://www.minecraft.net/eula",
        "https://microsoft.com/devicelogin",
        "https://www.microsoft.com/link",
    ]
    .contains(&url.as_str())
    {
        return Err("不允许打开此链接".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn open_folder(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: Option<String>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = if let Some(id) = id {
        if id.len() != 36 || !id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') {
            return Err("无效实例".into());
        }
        state.0.join("instances").join(id).join("game")
    } else {
        state.0.clone()
    };
    if !path.is_dir() {
        return Err("目录尚未创建".into());
    }
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn call(
    state: tauri::State<'_, AppState>,
    action: String,
    payload: Value,
) -> Result<Value, String> {
    let root = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        blocklink_service::rpc(&root, &action, payload).map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    #[cfg(windows)]
    if args.get(1).is_some_and(|a|a=="--apply-launcher-update"){
        if let (Some(target),Some(root))=(args.get(2),args.get(3)){if let Err(error)=updates::apply_windows(PathBuf::from(target),PathBuf::from(root)){let _=std::fs::write(PathBuf::from(root).join("update-error.txt"),error);}}
        return;
    }
    if args.get(1).is_some_and(|a| a == "--service") {
        let root = args
            .get(2)
            .map(PathBuf::from)
            .unwrap_or_else(blocklink_service::default_root);
        if let Err(e) = blocklink_service::serve(root) {
            eprintln!("{e:#}")
        }
        return;
    }
    let root = if args.get(1).is_some_and(|a| a == "--data-dir") {
        args.get(2)
            .map(PathBuf::from)
            .unwrap_or_else(blocklink_service::default_root)
    } else {
        blocklink_service::default_root()
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Pending(std::sync::Mutex::new(None)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            blocklink_service::ensure_service(&root)?;
            use tauri::Manager;
            app.manage(AppState(root.clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            call,
            pick_mod,
            pick_pack,
            save_pack,
            updates::check_update,
            updates::install_update,
            pick_world,
            open_link,
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("Blocklink 启动失败");
}
