use std::{
    collections::{BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(not(target_os = "macos"))]
use device_query::{DeviceQuery, DeviceState};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{broadcast, mpsc},
};

use crate::{
    logitech,
    model::{ActivityEntry, AppConfig, AppSnapshot},
    protocol::{self, Payload},
};

const PEER_STALE_MS: i64 = 4_000;
const MESSAGE_MAX_AGE_MS: i64 = 5_000;
const PEER_DISCOVERY_INTERVAL: Duration = Duration::from_secs(3);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_millis(600);
const TCP_FRAME_LIMIT: usize = 4_096;
const RECENT_CLAIM_LIMIT: usize = 128;

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
    let tcp_listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await.ok();
    if let Some(socket) = &socket {
        let _ = socket.set_broadcast(true);
    }
    if socket.is_some() || tcp_listener.is_some() {
        snapshot
            .write()
            .expect("snapshot poisoned")
            .diagnostics
            .network_listener = true;
    }
    let (tcp_outgoing, _) = broadcast::channel::<Vec<u8>>(64);
    let (tcp_incoming_sender, mut tcp_incoming) = mpsc::unbounded_channel::<Vec<u8>>();
    let active_tcp_connections = Arc::new(AtomicUsize::new(0));
    let tcp_scan_in_progress = Arc::new(AtomicBool::new(false));
    let mut packet = [0u8; 4_096];
    let mut heartbeat = tokio::time::interval(Duration::from_millis(900));
    let mut peer_discovery = tokio::time::interval(PEER_DISCOVERY_INTERVAL);
    peer_discovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut hardware_scan = tokio::time::interval(Duration::from_secs(2));
    let mut pending_since: Option<Instant> = None;
    let mut peer_addr: Option<SocketAddr> = None;
    let mut recent_claims = VecDeque::new();

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                mark_stale_peer(&snapshot);
                if pending_since.is_some_and(|at| at.elapsed() > Duration::from_millis(1_500)) {
                    snapshot.write().expect("snapshot poisoned").pending_claim = false;
                    pending_since = None;
                }
                send_payload(
                    socket.as_ref(),
                    &tcp_outgoing,
                    &snapshot,
                    heartbeat_payload(&snapshot),
                    peer_addr,
                ).await;
            }
            _ = peer_discovery.tick() => {
                let (peer_is_offline, platform) = {
                    let state = snapshot.read().expect("snapshot poisoned");
                    (!state.peer_host.online, state.config.platform.clone())
                };
                if peer_is_offline {
                    if matches!(platform, crate::model::Platform::Windows) {
                        start_tcp_discovery_sweep(
                            port,
                            tcp_outgoing.clone(),
                            tcp_incoming_sender.clone(),
                            active_tcp_connections.clone(),
                            tcp_scan_in_progress.clone(),
                        );
                    }
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
                        send_payload(
                            socket.as_ref(),
                            &tcp_outgoing,
                            &snapshot,
                            payload,
                            peer_addr,
                        ).await;
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
                            &tcp_outgoing,
                            Some(source),
                            &mut peer_addr,
                            &mut recent_claims,
                        ).await;
                    }
                }
            }
            accepted = async {
                match &tcp_listener {
                    Some(listener) => listener.accept().await.ok(),
                    None => std::future::pending().await,
                }
            } => {
                if let Some((stream, _)) = accepted {
                    spawn_tcp_connection(
                        stream,
                        tcp_outgoing.clone(),
                        tcp_incoming_sender.clone(),
                        active_tcp_connections.clone(),
                    );
                }
            }
            incoming = tcp_incoming.recv() => {
                if let Some(bytes) = incoming {
                    let code = snapshot.read().expect("snapshot poisoned").config.pairing_code.clone();
                    if let Ok(payload) = protocol::decode(&bytes, &code) {
                        handle_network_payload(
                            &snapshot,
                            payload,
                            socket.as_ref(),
                            &tcp_outgoing,
                            None,
                            &mut peer_addr,
                            &mut recent_claims,
                        ).await;
                    }
                }
            }
        }
    }
}

fn spawn_tcp_connection(
    stream: TcpStream,
    outgoing: broadcast::Sender<Vec<u8>>,
    incoming: mpsc::UnboundedSender<Vec<u8>>,
    active_connections: Arc<AtomicUsize>,
) {
    let mut outgoing = outgoing.subscribe();
    tauri::async_runtime::spawn(async move {
        active_connections.fetch_add(1, Ordering::Relaxed);
        let (mut reader, mut writer) = stream.into_split();
        loop {
            tokio::select! {
                frame = async {
                    let length = reader.read_u32().await? as usize;
                    if length == 0 || length > TCP_FRAME_LIMIT {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "TCP 消息长度无效",
                        ));
                    }
                    let mut bytes = vec![0; length];
                    reader.read_exact(&mut bytes).await?;
                    Ok::<Vec<u8>, std::io::Error>(bytes)
                } => match frame {
                    Ok(bytes) => {
                        if incoming.send(bytes).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                message = outgoing.recv() => match message {
                    Ok(bytes) => {
                        if writer.write_u32(bytes.len() as u32).await.is_err()
                            || writer.write_all(&bytes).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            }
        }
        active_connections.fetch_sub(1, Ordering::Relaxed);
    });
}

fn start_tcp_discovery_sweep(
    port: u16,
    outgoing: broadcast::Sender<Vec<u8>>,
    incoming: mpsc::UnboundedSender<Vec<u8>>,
    active_connections: Arc<AtomicUsize>,
    scan_in_progress: Arc<AtomicBool>,
) {
    if scan_in_progress.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut attempts = Vec::new();
        for target in tcp_discovery_targets(port) {
            attempts.push(tauri::async_runtime::spawn(async move {
                tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(target))
                    .await
                    .ok()
                    .and_then(Result::ok)
            }));
        }
        for attempt in attempts {
            if let Ok(Some(stream)) = attempt.await {
                spawn_tcp_connection(
                    stream,
                    outgoing.clone(),
                    incoming.clone(),
                    active_connections.clone(),
                );
            }
        }
        scan_in_progress.store(false, Ordering::Release);
    });
}

fn tcp_discovery_targets(port: u16) -> Vec<SocketAddrV4> {
    let interfaces = if_addrs::get_if_addrs().unwrap_or_default();
    let local_addresses: BTreeSet<Ipv4Addr> = interfaces
        .iter()
        .filter_map(|interface| match &interface.addr {
            if_addrs::IfAddr::V4(address) => Some(address.ip),
            if_addrs::IfAddr::V6(_) => None,
        })
        .collect();
    let mut targets = BTreeSet::new();
    for interface in interfaces.into_iter().filter(|interface| {
        interface.is_oper_up()
            && !interface.is_loopback()
            && !interface.is_link_local()
            && !interface.is_p2p()
            && is_likely_lan_interface(&interface.name)
    }) {
        let if_addrs::IfAddr::V4(address) = interface.addr else {
            continue;
        };
        targets.extend(subnet_probe_targets(address.ip, address.netmask, port));
    }
    targets.retain(|target| !local_addresses.contains(target.ip()));
    targets.into_iter().collect()
}

fn is_likely_lan_interface(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ![
        "bridge",
        "docker",
        "hyper-v",
        "meta",
        "tailscale",
        "utun",
        "vethernet",
        "virtualbox",
        "vmnet",
        "vmware",
        "wsl",
        "zerotier",
    ]
    .iter()
    .any(|marker| name.contains(marker))
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
    socket: Option<&UdpSocket>,
    tcp_outgoing: &broadcast::Sender<Vec<u8>>,
    snapshot: &Arc<RwLock<AppSnapshot>>,
    payload: Payload,
    peer_addr: Option<SocketAddr>,
) {
    let (code, port) = {
        let state = snapshot.read().expect("snapshot poisoned");
        (state.config.pairing_code.clone(), state.config.network_port)
    };
    if let Ok(bytes) = protocol::encode(payload, &code) {
        let _ = tcp_outgoing.send(bytes.clone());
        if let Some(socket) = socket {
            for target in discovery_targets(port) {
                let _ = socket.send_to(&bytes, target).await;
            }
            if let Some(peer_addr) = peer_addr {
                let _ = socket.send_to(&bytes, peer_addr).await;
            }
        }
    }
}

fn discovery_targets(port: u16) -> Vec<SocketAddrV4> {
    let broadcasts = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| {
            interface.is_oper_up()
                && !interface.is_loopback()
                && !interface.is_link_local()
                && !interface.is_p2p()
        })
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(address) => address.broadcast,
            if_addrs::IfAddr::V6(_) => None,
        });
    targets_from_broadcasts(port, broadcasts)
}

fn subnet_probe_targets(ip: Ipv4Addr, netmask: Ipv4Addr, port: u16) -> Vec<SocketAddrV4> {
    if !ip.is_private() {
        return Vec::new();
    }
    let prefix_length = u32::from(netmask).count_ones();
    if prefix_length > 30 {
        return Vec::new();
    }

    // 大网段只扫描本机所在的 /24，避免在企业或虚拟网络里产生大量探测包。
    let effective_prefix = prefix_length.max(24);
    let effective_mask = u32::MAX << (32 - effective_prefix);
    let network = u32::from(ip) & effective_mask;
    let broadcast = network | !effective_mask;
    ((network + 1)..broadcast)
        .map(Ipv4Addr::from)
        .filter(|candidate| *candidate != ip)
        .map(|candidate| SocketAddrV4::new(candidate, port))
        .collect()
}

fn targets_from_broadcasts(
    port: u16,
    broadcasts: impl IntoIterator<Item = Ipv4Addr>,
) -> Vec<SocketAddrV4> {
    let mut targets = BTreeSet::from([SocketAddrV4::new(Ipv4Addr::BROADCAST, port)]);
    targets.extend(
        broadcasts
            .into_iter()
            .filter(|address| !address.is_unspecified() && !address.is_loopback())
            .map(|address| SocketAddrV4::new(address, port)),
    );
    targets.into_iter().collect()
}

async fn handle_network_payload(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    payload: Payload,
    socket: Option<&UdpSocket>,
    tcp_outgoing: &broadcast::Sender<Vec<u8>>,
    udp_source: Option<SocketAddr>,
    peer_addr: &mut Option<SocketAddr>,
    recent_claims: &mut VecDeque<String>,
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
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
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
            nonce,
            sent_at_ms,
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
            if recent_claims.contains(&nonce) {
                return;
            }
            recent_claims.push_back(nonce);
            while recent_claims.len() > RECENT_CLAIM_LIMIT {
                recent_claims.pop_front();
            }
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
            let result = perform_switch(snapshot, channel);
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
            send_payload(socket, tcp_outgoing, snapshot, payload, *peer_addr).await;
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
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
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
        let (tcp_outgoing, _) = broadcast::channel(4);
        send_payload(
            Some(&sender),
            &tcp_outgoing,
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

    #[tokio::test]
    async fn tcp_connection_forwards_framed_messages_both_ways() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (outgoing, _) = broadcast::channel(4);
        let (incoming_sender, mut incoming) = mpsc::unbounded_channel();
        let active = Arc::new(AtomicUsize::new(0));

        spawn_tcp_connection(server, outgoing.clone(), incoming_sender, active.clone());
        tokio::time::timeout(Duration::from_secs(1), async {
            while active.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        client.write_u32(9).await.unwrap();
        client.write_all(b"from-peer").await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(1), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, b"from-peer");

        outgoing.send(b"to-peer".to_vec()).unwrap();
        let length = client.read_u32().await.unwrap();
        let mut received = vec![0; length as usize];
        client.read_exact(&mut received).await.unwrap();
        assert_eq!(received, b"to-peer");
    }

    #[tokio::test]
    async fn tcp_heartbeat_does_not_depend_on_udp_listener() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (outgoing, _) = broadcast::channel(4);
        let (incoming_sender, _) = mpsc::unbounded_channel();
        let active = Arc::new(AtomicUsize::new(0));
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));

        spawn_tcp_connection(server, outgoing.clone(), incoming_sender, active.clone());
        tokio::time::timeout(Duration::from_secs(1), async {
            while active.load(Ordering::Relaxed) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        send_payload(
            None,
            &outgoing,
            &snapshot,
            heartbeat_payload(&snapshot),
            None,
        )
        .await;

        let length = client.read_u32().await.unwrap();
        let mut received = vec![0; length as usize];
        client.read_exact(&mut received).await.unwrap();
        let decoded = protocol::decode(&received, "KEYCLAIM").unwrap();
        assert!(matches!(decoded, Payload::Heartbeat { channel: 3, .. }));
    }

    #[test]
    fn discovery_targets_include_every_interface_broadcast_once() {
        let targets = targets_from_broadcasts(
            45_823,
            [
                Ipv4Addr::new(192, 168, 1, 255),
                Ipv4Addr::new(192, 168, 75, 255),
                Ipv4Addr::new(192, 168, 1, 255),
            ],
        );

        assert_eq!(targets.len(), 3);
        assert!(targets.contains(&SocketAddrV4::new(Ipv4Addr::BROADCAST, 45_823)));
        assert!(targets.contains(&SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 255), 45_823,)));
        assert!(targets.contains(&SocketAddrV4::new(Ipv4Addr::new(192, 168, 75, 255), 45_823,)));
    }

    #[test]
    fn private_subnet_probe_covers_peer_without_self_or_broadcast() {
        let targets = subnet_probe_targets(
            Ipv4Addr::new(192, 168, 1, 178),
            Ipv4Addr::new(255, 255, 255, 0),
            45_823,
        );

        assert_eq!(targets.len(), 253);
        assert!(targets.contains(&SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 84), 45_823,)));
        assert!(!targets.contains(&SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 178), 45_823,)));
        assert!(!targets.contains(&SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 255), 45_823,)));
    }

    #[test]
    fn large_private_subnet_probe_is_capped_to_local_slash_24() {
        let targets = subnet_probe_targets(
            Ipv4Addr::new(172, 30, 64, 1),
            Ipv4Addr::new(255, 255, 240, 0),
            45_823,
        );

        assert_eq!(targets.len(), 253);
        assert!(targets.iter().all(|target| {
            let octets = target.ip().octets();
            octets[0..3] == [172, 30, 64]
        }));
    }

    #[test]
    fn public_interfaces_are_not_swept() {
        assert!(
            subnet_probe_targets(
                Ipv4Addr::new(203, 0, 113, 10),
                Ipv4Addr::new(255, 255, 255, 0),
                45_823,
            )
            .is_empty()
        );
    }

    #[test]
    fn tcp_discovery_skips_common_virtual_interfaces() {
        for name in [
            "vEthernet (WSL (Hyper-V firewall))",
            "VMware Network Adapter VMnet8",
            "Meta",
            "utun5",
        ] {
            assert!(!is_likely_lan_interface(name), "{name} 应被忽略");
        }
        assert!(is_likely_lan_interface("以太网"));
        assert!(is_likely_lan_interface("Wi-Fi"));
        assert!(is_likely_lan_interface("en7"));
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
