mod logitech;
mod model;
mod protocol;
mod runtime;

use model::{AppConfig, AppSnapshot};
use runtime::Runtime;
use tauri::{
    Manager, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

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

            let open_item = MenuItem::with_id(app, "open", "打开 KeyClaim", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出 KeyClaim", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            let mut tray_builder = TrayIconBuilder::new()
                .tooltip("KeyClaim · 键盘自动归属")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            tray_builder.build(app)?;

            if std::env::args().any(|argument| argument == "--autostart") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
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
