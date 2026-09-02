mod logitech;
mod model;
mod protocol;
mod runtime;

use model::{AppConfig, AppSnapshot};
use runtime::Runtime;
use tauri::{Manager, State};

#[tauri::command]
fn get_snapshot(runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.snapshot()
}

#[tauri::command]
fn refresh_hardware(runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.refresh_hardware();
    runtime.snapshot()
}

#[tauri::command]
fn test_recall(runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.request_claim();
    runtime.snapshot()
}

#[tauri::command]
fn switch_keyboard(channel: u8, runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.manual_switch(channel);
    runtime.snapshot()
}

#[tauri::command]
fn update_config(config: AppConfig, runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.update_config(config);
    runtime.snapshot()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(|app| {
            app.manage(Runtime::new(AppConfig::default()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh_hardware,
            test_recall,
            switch_keyboard,
            update_config
        ])
        .run(tauri::generate_context!())
        .expect("KeyClaim 启动失败");
}
