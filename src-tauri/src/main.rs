// Thin glue; compiled and smoke-tested in TAKO-004B. Keep it this small.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::Value;
use std::sync::Arc;
use tako_bridge::{Bridge, BridgeError};
use tauri::Manager;

/// The only command the UI can call. It forwards to the tested bridge on a blocking worker so the
/// UI thread is never blocked by git, processes or verification.
#[tauri::command]
async fn bridge_call(
    state: tauri::State<'_, Arc<Bridge>>,
    cmd: String,
    args: Value,
) -> Result<Value, BridgeError> {
    let bridge = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || bridge.dispatch(&cmd, args))
        .await
        .map_err(|e| BridgeError {
            code: "internal".into(),
            message: e.to_string(),
            details: vec![],
        })?
}

fn main() {
    // WebKitGTK's DMABUF renderer fails on some GPU/driver stacks ("Failed to create GBM buffer") and
    // leaves the window blank. Observed on the TAKO-004B test machine (Linux, GNOME). The software path
    // is slower but correct; respect an explicit user override.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(feature = "provider-auth-preview")]
            if app.config().identifier != "dev.tako.auth-preview" {
                return Err("Provider authentication preview requires its separate identifier/profile.".into());
            }
            // Opening the bridge also recovers any mission a previous crash left running.
            let dir = app.path().app_data_dir()?;
            #[cfg(not(feature = "native-qualification"))]
            let bridge = Bridge::open(dir).map_err(|e| e.message)?;
            #[cfg(feature = "native-qualification")]
            let bridge = {
                eprintln!("OFFLINE NATIVE QUALIFICATION: fixture adapter only; no provider network calls.");
                if let Some(window) = app.get_webview_window("main") {
                    window.set_title("TAKO — OFFLINE NATIVE QUALIFICATION")?;
                }
                Bridge::open_for_native_qualification(dir).map_err(|e| e.message)?
            };
            app.manage(Arc::new(bridge));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![bridge_call])
        .run(tauri::generate_context!())
        .expect("error while running TAKO");
}
