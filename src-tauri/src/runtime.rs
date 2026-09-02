use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

#[cfg(not(target_os = "macos"))]
use device_query::{DeviceQuery, DeviceState};
use tokio::{net::UdpSocket, sync::mpsc};

use crate::{
    logitech,
    model::{ActivityEntry, AppConfig, AppSnapshot},
    protocol::{self, Payload},
};

const PEER_STALE_MS: i64 = 4_000;
const MESSAGE_MAX_AGE_MS: i64 = 5_000;

enum LocalEvent {
    PointerMoved,
    RefreshHardware,
    ManualSwitch(u8),
}

#[derive(Clone)]
pub struct Runtime {
    snapshot: Arc<RwLock<AppSnapshot>>,
    events: mpsc::UnboundedSender<LocalEvent>,
}

impl Runtime {
    pub fn new(config: AppConfig) -> Self {
        let snapshot = Arc::new(RwLock::new(AppSnapshot::initial(config)));
        let (events, receiver) = mpsc::unbounded_channel();
        let runtime = Self { snapshot, events };
        runtime.start(receiver);
        runtime
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.snapshot.read().expect("snapshot poisoned").clone()
    }

    pub fn update_config(&self, config: AppConfig) {
        let mut snapshot = self.snapshot.write().expect("snapshot poisoned");
        snapshot.local_host.name = config.host_name.clone();
        snapshot.local_host.platform = config.platform.clone();
        snapshot.local_host.channel = config.channel;
        snapshot.peer_host.channel = config.peer_channel;
        snapshot.config = config;
    }

    pub fn refresh_hardware(&self) {
        let _ = self.events.send(LocalEvent::RefreshHardware);
    }

    pub fn manual_switch(&self, channel: u8) {
        let _ = self.events.send(LocalEvent::ManualSwitch(channel));
    }

    pub fn request_claim(&self) {
        let _ = self.events.send(LocalEvent::PointerMoved);
    }

    pub fn request_hid_access(&self) -> AppSnapshot {
        let granted = logitech::request_hid_access();
        {
            let mut snapshot = self.snapshot.write().expect("snapshot poisoned");
            snapshot.diagnostics.hid_access = granted || logitech::hid_access_granted();
            snapshot.local_host.ready = snapshot.diagnostics.hid_access;
            if !snapshot.diagnostics.hid_access {
                snapshot.keyboard.last_error =
                    Some("请在系统设置 → 隐私与安全性 → 输入监控中允许 KeyClaim".into());
            }
        }
        if granted {
            let _ = self.events.send(LocalEvent::RefreshHardware);
        }
        self.snapshot()
    }

    fn start(&self, receiver: mpsc::UnboundedReceiver<LocalEvent>) {
        start_input_monitor(self.snapshot.clone(), self.events.clone());
        tauri::async_runtime::spawn(run_loop(self.snapshot.clone(), receiver));
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn add_activity(snapshot: &mut AppSnapshot, title: &str, detail: String, kind: &str) {
    snapshot.activities.insert(
        0,
        ActivityEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            title: title.into(),
            detail,
            kind: kind.into(),
        },
    );
    snapshot.activities.truncate(8);
}

fn monitor_pointer<F>(
    snapshot: Arc<RwLock<AppSnapshot>>,
    events: mpsc::UnboundedSender<LocalEvent>,
    mut read_position: F,
) where
    F: FnMut() -> Option<(i32, i32)>,
{
    let Some(mut last) = read_position() else {
        return;
    };
    snapshot
        .write()
        .expect("snapshot poisoned")
        .diagnostics
        .input_monitor = true;
    let mut last_trigger = Instant::now() - Duration::from_secs(2);
    loop {
        std::thread::sleep(Duration::from_millis(16));
        let Some(current) = read_position() else {
            snapshot
                .write()
                .expect("snapshot poisoned")
                .diagnostics
                .input_monitor = false;
            continue;
        };
        let (threshold, debounce, enabled) = {
            let state = snapshot.read().expect("snapshot poisoned");
            (
                state.config.movement_threshold,
                state.config.debounce_ms,
                state.config.auto_recall,
            )
        };
        let distance = (current.0 - last.0).abs() + (current.1 - last.1).abs();
        last = current;
        if enabled
            && distance >= threshold
            && last_trigger.elapsed() >= Duration::from_millis(debounce)
        {
            last_trigger = Instant::now();
            let _ = events.send(LocalEvent::PointerMoved);
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn keyclaim_get_cursor_position(x: *mut i32, y: *mut i32) -> i32;
}

#[cfg(target_os = "macos")]
fn start_input_monitor(
    snapshot: Arc<RwLock<AppSnapshot>>,
    events: mpsc::UnboundedSender<LocalEvent>,
) {
    std::thread::spawn(move || {
        monitor_pointer(snapshot, events, || {
            let mut x = 0;
            let mut y = 0;
            // SAFETY: 原生函数只在调用期间写入两个有效的定长整数指针。
            let success = unsafe { keyclaim_get_cursor_position(&mut x, &mut y) };
            (success != 0).then_some((x, y))
        });
    });
}

#[cfg(not(target_os = "macos"))]
fn start_input_monitor(
    snapshot: Arc<RwLock<AppSnapshot>>,
    events: mpsc::UnboundedSender<LocalEvent>,
) {
    std::thread::spawn(move || {
        let Some(device_state) = DeviceState::checked_new() else {
            return;
        };
        monitor_pointer(snapshot, events, || Some(device_state.get_mouse().coords));
    });
}

async fn run_loop(
    snapshot: Arc<RwLock<AppSnapshot>>,
    mut events: mpsc::UnboundedReceiver<LocalEvent>,
) {
    let port = snapshot
        .read()
        .expect("snapshot poisoned")
        .config
        .network_port;
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).await.ok();
    if let Some(socket) = &socket {
        let _ = socket.set_broadcast(true);
        snapshot
            .write()
            .expect("snapshot poisoned")
            .diagnostics
            .network_listener = true;
    }
    let mut packet = [0u8; 4_096];
    let mut heartbeat = tokio::time::interval(Duration::from_millis(900));
    let mut hardware_scan = tokio::time::interval(Duration::from_secs(2));
    let mut pending_since: Option<Instant> = None;
    let mut peer_addr: Option<SocketAddr> = None;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                mark_stale_peer(&snapshot);
                if pending_since.is_some_and(|at| at.elapsed() > Duration::from_millis(1_500)) {
                    snapshot.write().expect("snapshot poisoned").pending_claim = false;
                    pending_since = None;
                }
                if let Some(socket) = &socket {
                    send_payload(socket, &snapshot, heartbeat_payload(&snapshot), peer_addr).await;
                }
            }
            _ = hardware_scan.tick() => refresh_keyboard(&snapshot).await,
            event = events.recv() => match event {
                Some(LocalEvent::PointerMoved) => {
                    let should_claim = {
                        let mut state = snapshot.write().expect("snapshot poisoned");
                        state.last_local_input_ms = Some(now_ms());
                        should_request_claim(&state)
                    };
                    if should_claim {
                        let payload = {
                            let mut state = snapshot.write().expect("snapshot poisoned");
                            state.pending_claim = true;
                            let channel = state.config.channel;
                            add_activity(
                                &mut state,
                                "检测到本地输入",
                                format!("正在向通道 {channel} 召回键盘"),
                                "claim",
                            );
                            Payload::Claim {
                                host_id: state.config.host_id.clone(),
                                channel: state.config.channel,
                                nonce: uuid::Uuid::new_v4().to_string(),
                                sent_at_ms: now_ms(),
                            }
                        };
                        pending_since = Some(Instant::now());
                        if let Some(socket) = &socket {
                            send_payload(socket, &snapshot, payload, peer_addr).await;
                        }
                    }
                }
                Some(LocalEvent::RefreshHardware) => refresh_keyboard(&snapshot).await,
                Some(LocalEvent::ManualSwitch(channel)) => {
                    let _ = perform_switch(&snapshot, channel);
                    refresh_keyboard(&snapshot).await;
                }
                None => break,
            },
            result = async {
                match &socket {
                    Some(socket) => socket.recv_from(&mut packet).await.ok(),
                    None => std::future::pending().await,
                }
            } => {
                if let Some((size, source)) = result {
                    let code = snapshot.read().expect("snapshot poisoned").config.pairing_code.clone();
                    if let Ok(payload) = protocol::decode(&packet[..size], &code) {
                        handle_network_payload(
                            &snapshot,
                            payload,
                            socket.as_ref(),
                            source,
                            &mut peer_addr,
                        ).await;
                    }
                }
            }
        }
    }
}

fn heartbeat_payload(snapshot: &Arc<RwLock<AppSnapshot>>) -> Payload {
    let state = snapshot.read().expect("snapshot poisoned");
    Payload::Heartbeat {
        host_id: state.config.host_id.clone(),
        host_name: state.config.host_name.clone(),
        platform: state.config.platform.clone(),
        channel: state.config.channel,
        ready: state.local_host.ready,
        keyboard_connected: state.keyboard.connected,
        current_channel: state.keyboard.current_channel,
        sent_at_ms: now_ms(),
    }
}

async fn send_payload(
    socket: &UdpSocket,
    snapshot: &Arc<RwLock<AppSnapshot>>,
    payload: Payload,
    peer_addr: Option<SocketAddr>,
) {
    let (code, port) = {
        let state = snapshot.read().expect("snapshot poisoned");
        (state.config.pairing_code.clone(), state.config.network_port)
    };
    if let Ok(bytes) = protocol::encode(payload, &code) {
        let target = SocketAddrV4::new(Ipv4Addr::BROADCAST, port);
        let _ = socket.send_to(&bytes, target).await;
        if let Some(peer_addr) = peer_addr {
            let _ = socket.send_to(&bytes, peer_addr).await;
        }
    }
}

async fn handle_network_payload(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    payload: Payload,
    socket: Option<&UdpSocket>,
    source: SocketAddr,
    peer_addr: &mut Option<SocketAddr>,
) {
    match payload {
        Payload::Heartbeat {
            host_id,
            host_name,
            platform,
            channel,
            ready,
            keyboard_connected,
            current_channel,
            sent_at_ms,
        } => {
            let mut state = snapshot.write().expect("snapshot poisoned");
            if host_id == state.config.host_id || (now_ms() - sent_at_ms).abs() > MESSAGE_MAX_AGE_MS
            {
                return;
            }
            *peer_addr = Some(source);
            let was_offline = !state.peer_host.online;
            state.peer_host.name = host_name;
            state.peer_host.platform = platform;
            state.peer_host.channel = channel;
            state.peer_host.online = true;
            state.peer_host.ready = ready;
            state.peer_host.last_seen_ms = Some(now_ms());
            record_peer_keyboard_channel(&mut state, keyboard_connected, current_channel);
            if was_offline {
                add_activity(
                    &mut state,
                    "另一台电脑已就绪",
                    "局域网握手成功".into(),
                    "network",
                );
            }
        }
        Payload::Claim {
            host_id,
            channel,
            sent_at_ms,
            ..
        } => {
            let allowed = {
                let state = snapshot.read().expect("snapshot poisoned");
                host_id != state.config.host_id
                    && state.peer_host.online
                    && state.peer_host.ready
                    && state.peer_host.channel == channel
                    && (now_ms() - sent_at_ms).abs() <= MESSAGE_MAX_AGE_MS
                    && state.keyboard.connected
                    && state.keyboard.current_channel == Some(state.config.channel)
            };
            if !allowed {
                return;
            }
            *peer_addr = Some(source);
            let result = perform_switch(snapshot, channel);
            if let Some(socket) = socket {
                let host_id = snapshot
                    .read()
                    .expect("snapshot poisoned")
                    .config
                    .host_id
                    .clone();
                let payload = Payload::SwitchResult {
                    host_id,
                    target_channel: channel,
                    success: result.is_ok(),
                    detail: result.err().unwrap_or_else(|| "键盘已切换".into()),
                    sent_at_ms: now_ms(),
                };
                send_payload(socket, snapshot, payload, *peer_addr).await;
            }
        }
        Payload::SwitchResult {
            host_id,
            target_channel,
            success,
            detail,
            sent_at_ms,
        } => {
            let mut state = snapshot.write().expect("snapshot poisoned");
            if host_id == state.config.host_id || (now_ms() - sent_at_ms).abs() > MESSAGE_MAX_AGE_MS
            {
                return;
            }
            *peer_addr = Some(source);
            state.pending_claim = false;
            if success {
                state.keyboard.current_channel = Some(target_channel);
                add_activity(&mut state, "键盘已归位", detail, "success");
            } else {
                add_activity(&mut state, "召回未完成", detail, "error");
            }
        }
    }
}

fn perform_switch(snapshot: &Arc<RwLock<AppSnapshot>>, channel: u8) -> Result<(), String> {
    match logitech::switch_to(channel) {
        Ok(()) => {
            let mut state = snapshot.write().expect("snapshot poisoned");
            state.keyboard.current_channel = Some(channel);
            add_activity(
                &mut state,
                "已发送切换指令",
                format!("MX Keys → 通道 {channel}"),
                "success",
            );
            Ok(())
        }
        Err(error) => {
            let detail = error.to_string();
            let mut state = snapshot.write().expect("snapshot poisoned");
            state.keyboard.last_error = Some(detail.clone());
            add_activity(&mut state, "切换失败", detail.clone(), "error");
            Err(detail)
        }
    }
}

async fn refresh_keyboard(snapshot: &Arc<RwLock<AppSnapshot>>) {
    let hid_access = logitech::hid_access_granted();
    if !hid_access {
        let mut state = snapshot.write().expect("snapshot poisoned");
        state.diagnostics.hid_access = false;
        state.diagnostics.receiver_probe = false;
        state.local_host.ready = false;
        state.keyboard.connected = false;
        state.keyboard.current_channel = None;
        state.keyboard.supports_change_host = false;
        state.keyboard.last_error =
            Some("请在系统设置 → 隐私与安全性 → 输入监控中允许 KeyClaim".into());
        return;
    }
    let result = tokio::task::spawn_blocking(logitech::probe).await;
    let mut state = snapshot.write().expect("snapshot poisoned");
    state.diagnostics.hid_access = true;
    state.diagnostics.receiver_probe = true;
    state.local_host.ready = true;
    match result {
        Ok(Ok(found)) => {
            state.keyboard.connected = true;
            state.keyboard.receiver_name = Some(found.receiver_name);
            state.keyboard.current_channel = Some(found.current_channel);
            state.keyboard.supports_change_host = found.channel_count > 1;
            state.keyboard.last_error = None;
        }
        Ok(Err(error)) => {
            state.keyboard.connected = false;
            state.keyboard.current_channel = None;
            state.keyboard.supports_change_host = false;
            state.keyboard.last_error = Some(error.to_string());
        }
        Err(error) => {
            state.keyboard.connected = false;
            state.keyboard.current_channel = None;
            state.keyboard.last_error = Some(format!("硬件扫描任务失败: {error}"));
        }
    }
}

fn should_request_claim(state: &AppSnapshot) -> bool {
    state.config.auto_recall
        && state.peer_host.online
        && state.peer_host.ready
        && state.keyboard.current_channel != Some(state.config.channel)
        && !state.pending_claim
}

fn record_peer_keyboard_channel(
    state: &mut AppSnapshot,
    keyboard_connected: bool,
    current_channel: Option<u8>,
) {
    if keyboard_connected && current_channel.is_some() {
        state.keyboard.current_channel = current_channel;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppConfig, AppSnapshot, Platform};

    fn mac_snapshot() -> AppSnapshot {
        let config = AppConfig {
            platform: Platform::Macos,
            channel: 3,
            peer_channel: 2,
            ..AppConfig::default()
        };
        let mut state = AppSnapshot::initial(config);
        state.peer_host.online = true;
        state.peer_host.ready = true;
        state
    }

    #[test]
    fn unknown_channel_requests_keyboard_from_ready_peer() {
        let mut state = mac_snapshot();
        state.keyboard.current_channel = None;
        assert!(should_request_claim(&state));
    }

    #[test]
    fn local_channel_does_not_request_keyboard_again() {
        let mut state = mac_snapshot();
        state.keyboard.current_channel = Some(3);
        assert!(!should_request_claim(&state));
    }

    #[test]
    fn peer_heartbeat_corrects_stale_keyboard_channel() {
        let mut state = mac_snapshot();
        state.keyboard.current_channel = Some(3);
        record_peer_keyboard_channel(&mut state, true, Some(2));
        assert_eq!(state.keyboard.current_channel, Some(2));
        assert!(should_request_claim(&state));
    }

    #[test]
    fn disconnected_peer_does_not_overwrite_known_channel() {
        let mut state = mac_snapshot();
        state.keyboard.current_channel = Some(3);
        record_peer_keyboard_channel(&mut state, false, None);
        assert_eq!(state.keyboard.current_channel, Some(3));
    }

    #[tokio::test]
    async fn known_peer_receives_direct_heartbeat() {
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let peer_addr = receiver.local_addr().unwrap();
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        sender.set_broadcast(true).unwrap();

        let mut state = mac_snapshot();
        state.config.network_port = 59_999;
        let snapshot = Arc::new(RwLock::new(state));
        send_payload(
            &sender,
            &snapshot,
            heartbeat_payload(&snapshot),
            Some(peer_addr),
        )
        .await;

        let mut packet = [0u8; 4_096];
        let (size, _) =
            tokio::time::timeout(Duration::from_secs(1), receiver.recv_from(&mut packet))
                .await
                .expect("direct heartbeat timed out")
                .unwrap();
        let decoded = protocol::decode(&packet[..size], "KEYCLAIM").unwrap();
        assert!(matches!(decoded, Payload::Heartbeat { channel: 3, .. }));
    }
}

fn mark_stale_peer(snapshot: &Arc<RwLock<AppSnapshot>>) {
    let mut state = snapshot.write().expect("snapshot poisoned");
    let stale = state
        .peer_host
        .last_seen_ms
        .is_none_or(|seen| now_ms() - seen > PEER_STALE_MS);
    if stale {
        state.peer_host.online = false;
        state.peer_host.ready = false;
    }
}
