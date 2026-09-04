mod input;
mod logitech;
mod model;
mod protocol;
mod runtime;

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use model::{AppConfig, AppSnapshot};
use runtime::Runtime;
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, State,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[cfg(not(target_os = "macos"))]
use tauri::menu::{Menu, MenuItem};

const POPOVER_EDGE_INSET: i32 = 8;
const POPOVER_GAP: i32 = 2;
const INITIAL_FOCUS_GRACE: Duration = Duration::from_millis(350);
const TRAY_PRESS_GRACE: Duration = Duration::from_secs(1);
const TRAY_FOCUS_RACE_GRACE: Duration = Duration::from_millis(250);

#[derive(Default)]
struct PopoverState {
    anchor: Mutex<Option<Rect>>,
    shown_at: Mutex<Option<Instant>>,
    tray_press: Mutex<Option<(Instant, bool)>>,
    focus_hidden_at: Mutex<Option<Instant>>,
}

fn clamp_popover_position(
    tray_position: PhysicalPosition<i32>,
    tray_size: PhysicalSize<u32>,
    window_size: PhysicalSize<u32>,
    work_position: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    scale_factor: f64,
) -> PhysicalPosition<i32> {
    let tray_center_x = tray_position.x + tray_size.width as i32 / 2;
    let desired_x = tray_center_x - window_size.width as i32 / 2;
    let inset = (POPOVER_EDGE_INSET as f64 * scale_factor).round() as i32;
    let gap = (POPOVER_GAP as f64 * scale_factor).round() as i32;
    let min_x = work_position.x + inset;
    let max_x =
        (work_position.x + work_size.width as i32 - window_size.width as i32 - inset).max(min_x);
    let min_y = work_position.y;
    let max_y = (work_position.y + work_size.height as i32 - window_size.height as i32).max(min_y);

    PhysicalPosition::new(
        desired_x.clamp(min_x, max_x),
        (tray_position.y + tray_size.height as i32 + gap).clamp(min_y, max_y),
    )
}

fn position_main_window(window: &tauri::WebviewWindow, anchor: Rect) {
    let Ok(scale_factor) = window.scale_factor() else {
        return;
    };
    let tray_position = anchor.position.to_physical::<i32>(scale_factor);
    let tray_size = anchor.size.to_physical::<u32>(scale_factor);
    let Ok(window_size) = window.outer_size() else {
        return;
    };
    let tray_center = PhysicalPosition::new(
        tray_position.x + tray_size.width as i32 / 2,
        tray_position.y + tray_size.height as i32 / 2,
    );

    let Ok(monitors) = window.available_monitors() else {
        return;
    };
    let monitor = monitors.iter().find(|monitor| {
        let position = monitor.position();
        let size = monitor.size();
        tray_center.x >= position.x
            && tray_center.x < position.x + size.width as i32
            && tray_center.y >= position.y
            && tray_center.y < position.y + size.height as i32
    });

    if let Some(monitor) = monitor.or_else(|| monitors.first()) {
        let work_area = monitor.work_area();
        let position = clamp_popover_position(
            tray_position,
            tray_size,
            window_size,
            work_area.position,
            work_area.size,
            monitor.scale_factor(),
        );
        let _ = window.set_position(position);
    }
}

fn remember_tray_anchor(app: &tauri::AppHandle, anchor: Rect) {
    if let Some(state) = app.try_state::<PopoverState>() {
        if let Ok(mut stored) = state.anchor.lock() {
            *stored = Some(anchor);
        }
    }
}

fn stored_tray_anchor(app: &tauri::AppHandle) -> Option<Rect> {
    app.try_state::<PopoverState>()
        .and_then(|state| state.anchor.lock().ok().and_then(|stored| *stored))
}

fn mark_popover_shown(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<PopoverState>() {
        if let Ok(mut shown_at) = state.shown_at.lock() {
            *shown_at = Some(Instant::now());
        }
    }
}

fn is_initial_focus_transition(app: &tauri::AppHandle) -> bool {
    app.try_state::<PopoverState>()
        .and_then(|state| {
            state.shown_at.lock().ok().and_then(|shown_at| {
                shown_at.map(|instant| instant.elapsed() < INITIAL_FOCUS_GRACE)
            })
        })
        .unwrap_or(false)
}

fn begin_tray_press(app: &tauri::AppHandle) {
    let currently_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if let Some(state) = app.try_state::<PopoverState>() {
        let recently_focus_hidden = state
            .focus_hidden_at
            .lock()
            .ok()
            .and_then(|mut hidden_at| hidden_at.take())
            .is_some_and(|instant| instant.elapsed() < TRAY_FOCUS_RACE_GRACE);
        if let Ok(mut tray_press) = state.tray_press.lock() {
            *tray_press = Some((Instant::now(), currently_visible || recently_focus_hidden));
        }
    }
}

fn tray_press_in_progress(app: &tauri::AppHandle) -> bool {
    app.try_state::<PopoverState>()
        .and_then(|state| {
            state.tray_press.lock().ok().and_then(|tray_press| {
                tray_press.map(|(instant, _)| instant.elapsed() < TRAY_PRESS_GRACE)
            })
        })
        .unwrap_or(false)
}

fn take_tray_press_visibility(app: &tauri::AppHandle) -> Option<bool> {
    app.try_state::<PopoverState>().and_then(|state| {
        state
            .tray_press
            .lock()
            .ok()
            .and_then(|mut tray_press| tray_press.take())
            .and_then(|(instant, was_visible)| {
                (instant.elapsed() < TRAY_PRESS_GRACE).then_some(was_visible)
            })
    })
}

fn show_main_window(app: &tauri::AppHandle, anchor: Option<Rect>) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        {
            if let Some(anchor) = anchor.or_else(|| stored_tray_anchor(app)) {
                position_main_window(&window, anchor);
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = anchor;
        let _ = window.unminimize();
        if let Some(state) = app.try_state::<PopoverState>() {
            if let Ok(mut hidden_at) = state.focus_hidden_at.lock() {
                *hidden_at = None;
            }
        }
        mark_popover_shown(app);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("popover-hidden", ());
        let _ = window.hide();
    }
}

fn mark_focus_hidden(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<PopoverState>() {
        if let Ok(mut hidden_at) = state.focus_hidden_at.lock() {
            *hidden_at = Some(Instant::now());
        }
    }
}

fn finish_tray_click(app: &tauri::AppHandle, anchor: Rect) {
    remember_tray_anchor(app, anchor);
    let was_visible = take_tray_press_visibility(app).unwrap_or_else(|| {
        app.get_webview_window("main")
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false)
    });
    if was_visible {
        hide_main_window(app);
    } else {
        show_main_window(app, Some(anchor));
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
fn request_hid_access(runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.request_hid_access()
}

#[tauri::command]
fn update_config(config: AppConfig, runtime: State<'_, Runtime>) -> AppSnapshot {
    runtime.update_config(config);
    runtime.snapshot()
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app, None);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::AppleScript,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            app.manage(Runtime::new(AppConfig::default()));
            app.manage(PopoverState::default());

            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            let mut tray_builder = TrayIconBuilder::new()
                .tooltip("KeyClaim · 键来｜键盘自动跟随")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        rect,
                        button,
                        button_state,
                        ..
                    } = event
                    {
                        remember_tray_anchor(tray.app_handle(), rect);
                        if button == MouseButton::Left {
                            #[cfg(target_os = "macos")]
                            {
                                match button_state {
                                    MouseButtonState::Down => begin_tray_press(tray.app_handle()),
                                    MouseButtonState::Up => {
                                        finish_tray_click(tray.app_handle(), rect)
                                    }
                                }
                            }
                            #[cfg(not(target_os = "macos"))]
                            if button_state == MouseButtonState::Up {
                                show_main_window(tray.app_handle(), None);
                            }
                        }
                    }
                });

            #[cfg(not(target_os = "macos"))]
            {
                let open_item = MenuItem::with_id(app, "open", "打开主界面", true, None::<&str>)?;
                let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
                let tray_menu = Menu::with_items(app, &[&open_item, &quit_item])?;
                tray_builder =
                    tray_builder.menu(&tray_menu).on_menu_event(|app, event| {
                        match event.id.as_ref() {
                            "open" => show_main_window(app, None),
                            "quit" => app.exit(0),
                            _ => {}
                        }
                    });
            }

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            tray_builder.build(app)?;

            #[cfg(target_os = "windows")]
            if !std::env::args().any(|argument| argument == "--hidden") {
                show_main_window(app.handle(), None);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                hide_main_window(window.app_handle());
            }
            tauri::WindowEvent::Focused(false) => {
                #[cfg(target_os = "macos")]
                {
                    if !is_initial_focus_transition(window.app_handle())
                        && !tray_press_in_progress(window.app_handle())
                    {
                        mark_focus_hidden(window.app_handle());
                        hide_main_window(window.app_handle());
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            refresh_hardware,
            test_recall,
            switch_keyboard,
            request_hid_access,
            update_config,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("KeyClaim 启动失败");
}

#[cfg(test)]
mod window_tests {
    use super::*;

    #[test]
    fn popover_is_centered_below_the_tray_icon() {
        let position = clamp_popover_position(
            PhysicalPosition::new(1000, 0),
            PhysicalSize::new(48, 48),
            PhysicalSize::new(840, 436),
            PhysicalPosition::new(0, 48),
            PhysicalSize::new(1920, 1032),
            2.0,
        );

        assert_eq!(position, PhysicalPosition::new(604, 52));
    }

    #[test]
    fn popover_stays_inside_the_right_screen_edge() {
        let position = clamp_popover_position(
            PhysicalPosition::new(1890, 0),
            PhysicalSize::new(48, 48),
            PhysicalSize::new(840, 436),
            PhysicalPosition::new(0, 48),
            PhysicalSize::new(1920, 1032),
            2.0,
        );

        assert_eq!(position, PhysicalPosition::new(1064, 52));
    }

    #[test]
    fn popover_uses_the_selected_monitor_origin() {
        let position = clamp_popover_position(
            PhysicalPosition::new(-80, 0),
            PhysicalSize::new(48, 48),
            PhysicalSize::new(840, 436),
            PhysicalPosition::new(-1920, 48),
            PhysicalSize::new(1920, 1032),
            2.0,
        );

        assert_eq!(position, PhysicalPosition::new(-856, 52));
    }
}
