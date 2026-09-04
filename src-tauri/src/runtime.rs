use std::{
    collections::{BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(not(test))]
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::OnceLock,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{broadcast, mpsc},
};

use crate::{
    input::{self, InputIntentGate, PointerActivity},
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
const RECENT_MESSAGE_LIMIT: usize = 256;
const CLAIM_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const CLAIM_TIMEOUT: Duration = Duration::from_millis(1_500);
const CLAIM_MAX_ATTEMPTS: u8 = 4;
const CLAIM_DUPLICATE_WINDOW: Duration = Duration::from_millis(500);

enum LocalEvent {
    PointerActivity(PointerActivity),
    InputMonitorStatus(bool),
    RefreshHardware,
    HardwareProbeCompleted(HardwareProbeOutcome),
    HardwareSwitchCompleted(HardwareSwitchOutcome),
    ManualSwitch(u8),
}

#[derive(Debug)]
struct HardwareProbeOutcome {
    hid_access: bool,
    result: Result<logitech::ProbeResult, String>,
}

#[derive(Debug)]
enum HardwareSwitchOrigin {
    RemoteClaim { nonce: String },
    Manual,
}

#[derive(Debug)]
struct HardwareSwitchOutcome {
    origin: HardwareSwitchOrigin,
    channel: u8,
    result: Result<String, String>,
}

#[derive(Clone, Debug)]
struct PendingClaim {
    nonce: String,
    channel: u8,
    started_at: Instant,
    last_sent_at: Instant,
    attempts: u8,
    dispatch_reported: bool,
    last_failure_detail: Option<String>,
}

#[derive(Clone, Debug)]
struct ActiveRemoteSwitch {
    primary_nonce: String,
    channel: u8,
    coalesced_nonces: Vec<String>,
}

impl ActiveRemoteSwitch {
    fn new(primary_nonce: String, channel: u8) -> Self {
        Self {
            primary_nonce,
            channel,
            coalesced_nonces: Vec::new(),
        }
    }

    fn contains(&self, nonce: &str) -> bool {
        self.primary_nonce == nonce
            || self
                .coalesced_nonces
                .iter()
                .any(|candidate| candidate == nonce)
    }

    fn coalesce(&mut self, nonce: String) {
        if !self.contains(&nonce) {
            self.coalesced_nonces.push(nonce);
        }
    }

    fn all_nonces(self) -> impl Iterator<Item = String> {
        std::iter::once(self.primary_nonce).chain(self.coalesced_nonces)
    }
}

#[derive(Clone, Debug)]
struct RecentClaim {
    nonce: String,
    handled_at: Instant,
    accepted: bool,
    detail: String,
}

#[derive(Debug)]
struct RecentMessage {
    bytes: Vec<u8>,
    received_at: Instant,
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
        let _ = self
            .events
            .send(LocalEvent::PointerActivity(PointerActivity {
                delta_x: i32::MAX,
                delta_y: 0,
                synthetic: false,
            }));
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
        let input_events = self.events.clone();
        let status_events = self.events.clone();
        input::start_monitor(
            move |activity| {
                let _ = input_events.send(LocalEvent::PointerActivity(activity));
            },
            move |ready| {
                let _ = status_events.send(LocalEvent::InputMonitorStatus(ready));
            },
        );
        tauri::async_runtime::spawn(run_loop(
            self.snapshot.clone(),
            receiver,
            self.events.clone(),
        ));
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn add_activity(snapshot: &mut AppSnapshot, title: &str, detail: String, kind: &str) {
    #[cfg(not(test))]
    write_diagnostic_log(kind, title, &detail);
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

#[cfg(not(test))]
fn diagnostic_log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        dirs::data_local_dir().map(|root| root.join("KeyClaim").join("keyclaim.log"))
    })
    .as_ref()
}

#[cfg(not(test))]
fn write_diagnostic_log(kind: &str, title: &str, detail: &str) {
    let Some(path) = diagnostic_log_path() else {
        return;
    };
    if let Some(parent) = path.parent()
        && fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if let Ok(metadata) = fs::metadata(path)
        && metadata.len() > 1_000_000
    {
        let _ = fs::rename(path, path.with_extension("log.previous"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{}\t{}\t{}\t{}",
            chrono::Local::now().to_rfc3339(),
            kind,
            title.replace(['\t', '\n'], " "),
            detail.replace(['\t', '\n'], " ")
        );
    }
}

async fn run_loop(
    snapshot: Arc<RwLock<AppSnapshot>>,
    mut events: mpsc::UnboundedReceiver<LocalEvent>,
    event_sender: mpsc::UnboundedSender<LocalEvent>,
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
    let mut claim_retry = tokio::time::interval(Duration::from_millis(50));
    claim_retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut peer_discovery = tokio::time::interval(PEER_DISCOVERY_INTERVAL);
    peer_discovery.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut hardware_scan = tokio::time::interval(Duration::from_millis(500));
    hardware_scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let hardware_probe_in_progress = Arc::new(AtomicBool::new(false));
    let mut pending_claim: Option<PendingClaim> = None;
    let mut intent_gate = InputIntentGate::new();
    let mut peer_addr: Option<SocketAddr> = None;
    let mut recent_claims = VecDeque::new();
    let mut recent_messages = VecDeque::new();
    let mut active_remote_switch: Option<ActiveRemoteSwitch> = None;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                mark_stale_peer(&snapshot);
                send_payload(
                    socket.as_ref(),
                    &tcp_outgoing,
                    &snapshot,
                    heartbeat_payload(&snapshot),
                    peer_addr,
                ).await;
            }
            _ = claim_retry.tick() => {
                if let Some(payload) = drive_pending_claim(&snapshot, &mut pending_claim) {
                    send_payload(
                        socket.as_ref(),
                        &tcp_outgoing,
                        &snapshot,
                        payload,
                        peer_addr,
                    ).await;
                }
            }
            _ = peer_discovery.tick() => {
                let platform = {
                    let state = snapshot.read().expect("snapshot poisoned");
                    state.config.platform.clone()
                };
                if should_start_tcp_discovery(
                    active_tcp_connections.load(Ordering::Relaxed),
                ) {
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
            _ = hardware_scan.tick() => {
                start_hardware_probe(
                    &event_sender,
                    &hardware_probe_in_progress,
                );
            },
            event = events.recv() => match event {
                Some(LocalEvent::HardwareProbeCompleted(outcome)) => {
                    hardware_probe_in_progress.store(false, Ordering::Release);
                    let local_channel = apply_hardware_probe(&snapshot, outcome);
                    let confirmed_arrival = pending_claim.as_ref().and_then(|claim| {
                        (local_channel == Some(claim.channel))
                            .then(|| (claim.channel, claim.nonce.clone()))
                    });
                    if let Some((channel, claim_nonce)) = confirmed_arrival {
                        pending_claim = None;
                        {
                            let mut state = snapshot.write().expect("snapshot poisoned");
                            state.pending_claim = false;
                            add_activity(
                                &mut state,
                                "键盘已归位",
                                format!("目标端已确认 MX Keys 上线 · 通道 {channel}"),
                                "success",
                            );
                        }
                        let host_id = snapshot
                            .read()
                            .expect("snapshot poisoned")
                            .config
                            .host_id
                            .clone();
                        send_payload(
                            socket.as_ref(),
                            &tcp_outgoing,
                            &snapshot,
                            Payload::ArrivalConfirmed {
                                host_id,
                                claim_nonce,
                                channel,
                                sent_at_ms: now_ms(),
                            },
                            peer_addr,
                        ).await;
                    }
                }
                Some(LocalEvent::HardwareSwitchCompleted(outcome)) => {
                    match outcome.origin {
                        HardwareSwitchOrigin::RemoteClaim { nonce } => {
                            let (accepted, detail) = match outcome.result {
                                Ok(detail) => (true, detail),
                                Err(detail) => (false, detail),
                            };
                            let Some(active) = active_remote_switch.take().filter(|active| {
                                active.primary_nonce == nonce && active.channel == outcome.channel
                            }) else {
                                continue;
                            };
                            let host_id = snapshot
                                .read()
                                .expect("snapshot poisoned")
                                .config
                                .host_id
                                .clone();
                            for claim_nonce in active.all_nonces() {
                                remember_claim_result(
                                    &mut recent_claims,
                                    claim_nonce.clone(),
                                    accepted,
                                    detail.clone(),
                                );
                                send_payload(
                                    socket.as_ref(),
                                    &tcp_outgoing,
                                    &snapshot,
                                    Payload::SwitchDispatched {
                                        host_id: host_id.clone(),
                                        claim_nonce,
                                        target_channel: outcome.channel,
                                        accepted,
                                        detail: detail.clone(),
                                        sent_at_ms: now_ms(),
                                    },
                                    peer_addr,
                                ).await;
                            }
                        }
                        HardwareSwitchOrigin::Manual => {}
                    }
                }
                Some(LocalEvent::PointerActivity(activity)) => {
                    let (threshold, debounce, ignore_synthetic) = {
                        let state = snapshot.read().expect("snapshot poisoned");
                        (
                            state.config.movement_threshold,
                            Duration::from_millis(state.config.debounce_ms),
                            state.config.ignore_synthetic_input,
                        )
                    };
                    if !intent_gate.observe(
                        activity,
                        threshold,
                        debounce,
                        ignore_synthetic,
                        Instant::now(),
                    ) {
                        continue;
                    }
                    let should_claim = {
                        let mut state = snapshot.write().expect("snapshot poisoned");
                        state.last_local_input_ms = Some(now_ms());
                        should_request_claim(&state)
                    };
                    if should_claim {
                        let (pending, payload) = begin_claim(&snapshot, "检测到本地物理输入");
                        pending_claim = Some(pending);
                        send_payload(
                            socket.as_ref(),
                            &tcp_outgoing,
                            &snapshot,
                            payload,
                            peer_addr,
                        ).await;
                    }
                }
                Some(LocalEvent::InputMonitorStatus(ready)) => {
                    let mut state = snapshot.write().expect("snapshot poisoned");
                    let was_ready = state.diagnostics.input_monitor;
                    if was_ready == ready {
                        continue;
                    }
                    state.diagnostics.input_monitor = ready;
                    if ready {
                        add_activity(
                            &mut state,
                            "输入监听已就绪",
                            "正在监听本机真实鼠标与触控板事件".into(),
                            "success",
                        );
                    } else if was_ready {
                        add_activity(
                            &mut state,
                            "输入监听未就绪",
                            "系统原生输入事件流将在后台自动重试".into(),
                            "error",
                        );
                    }
                }
                Some(LocalEvent::RefreshHardware) => {
                    start_hardware_probe(
                        &event_sender,
                        &hardware_probe_in_progress,
                    );
                }
                Some(LocalEvent::ManualSwitch(channel)) => {
                    let local_channel = snapshot.read().expect("snapshot poisoned").config.channel;
                    if channel == local_channel {
                        let (pending, payload) = begin_claim(&snapshot, "手动召回键盘");
                        pending_claim = Some(pending);
                        send_payload(
                            socket.as_ref(),
                            &tcp_outgoing,
                            &snapshot,
                            payload,
                            peer_addr,
                        ).await;
                    } else {
                        start_hardware_switch(
                            &event_sender,
                            &snapshot,
                            channel,
                            HardwareSwitchOrigin::Manual,
                        );
                    }
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
                    if let Some(payload) = decode_new_payload(
                        &packet[..size],
                        &code,
                        &mut recent_messages,
                    ) {
                        handle_network_payload(
                            &snapshot,
                            payload,
                            socket.as_ref(),
                            &tcp_outgoing,
                            Some(source),
                            &mut peer_addr,
                            &mut recent_claims,
                            &mut pending_claim,
                            &event_sender,
                            &mut active_remote_switch,
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
                    if let Some(payload) = decode_new_payload(
                        &bytes,
                        &code,
                        &mut recent_messages,
                    ) {
                        handle_network_payload(
                            &snapshot,
                            payload,
                            socket.as_ref(),
                            &tcp_outgoing,
                            None,
                            &mut peer_addr,
                            &mut recent_claims,
                            &mut pending_claim,
                            &event_sender,
                            &mut active_remote_switch,
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
        let read_loop = async {
            loop {
                let frame = async {
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
                }
                .await;
                match frame {
                    Ok(bytes) => {
                        if incoming.send(bytes).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        };
        let write_loop = async {
            loop {
                match outgoing.recv().await {
                    Ok(bytes) => {
                        if writer.write_u32(bytes.len() as u32).await.is_err()
                            || writer.write_all(&bytes).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        tokio::select! {
            _ = read_loop => {},
            _ = write_loop => {},
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
        let mut selected_stream = None;
        for attempt in attempts {
            if let Ok(Some(stream)) = attempt.await {
                retain_first(&mut selected_stream, stream);
            }
        }
        if let Some(stream) = selected_stream {
            spawn_tcp_connection(stream, outgoing, incoming, active_connections);
        }
        scan_in_progress.store(false, Ordering::Release);
    });
}

fn retain_first<T>(selected: &mut Option<T>, candidate: T) {
    if selected.is_none() {
        *selected = Some(candidate);
    }
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

fn decode_new_payload(
    bytes: &[u8],
    pairing_code: &str,
    recent_messages: &mut VecDeque<RecentMessage>,
) -> Option<Payload> {
    let payload = protocol::decode(bytes, pairing_code).ok()?;
    while recent_messages.front().is_some_and(|message| {
        message.received_at.elapsed() > Duration::from_millis(MESSAGE_MAX_AGE_MS as u64)
    }) {
        recent_messages.pop_front();
    }
    if recent_messages.iter().any(|message| message.bytes == bytes) {
        return None;
    }
    recent_messages.push_back(RecentMessage {
        bytes: bytes.to_vec(),
        received_at: Instant::now(),
    });
    while recent_messages.len() > RECENT_MESSAGE_LIMIT {
        recent_messages.pop_front();
    }
    Some(payload)
}

async fn handle_network_payload(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    payload: Payload,
    socket: Option<&UdpSocket>,
    tcp_outgoing: &broadcast::Sender<Vec<u8>>,
    udp_source: Option<SocketAddr>,
    peer_addr: &mut Option<SocketAddr>,
    recent_claims: &mut VecDeque<RecentClaim>,
    pending_claim: &mut Option<PendingClaim>,
    event_sender: &mpsc::UnboundedSender<LocalEvent>,
    active_remote_switch: &mut Option<ActiveRemoteSwitch>,
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
                claim_is_authorized(&state, &host_id, channel, sent_at_ms)
            };
            if !allowed {
                return;
            }
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
            let Some((accepted, detail)) = cached_claim_result(recent_claims, &nonce) else {
                cancel_pending_claim(snapshot, pending_claim);
                if let Some(active) = active_remote_switch.as_mut() {
                    active.coalesce(nonce);
                    return;
                }
                *active_remote_switch = Some(ActiveRemoteSwitch::new(nonce.clone(), channel));
                start_hardware_switch(
                    event_sender,
                    snapshot,
                    channel,
                    HardwareSwitchOrigin::RemoteClaim { nonce },
                );
                return;
            };
            let host_id = snapshot
                .read()
                .expect("snapshot poisoned")
                .config
                .host_id
                .clone();
            let payload = Payload::SwitchDispatched {
                host_id,
                claim_nonce: nonce,
                target_channel: channel,
                accepted,
                detail,
                sent_at_ms: now_ms(),
            };
            send_payload(socket, tcp_outgoing, snapshot, payload, *peer_addr).await;
        }
        Payload::SwitchDispatched {
            host_id,
            claim_nonce,
            target_channel,
            accepted,
            detail,
            sent_at_ms,
        } => {
            let state = snapshot.read().expect("snapshot poisoned");
            if host_id == state.config.host_id || (now_ms() - sent_at_ms).abs() > MESSAGE_MAX_AGE_MS
            {
                return;
            }
            drop(state);
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
            let Some(pending) = pending_claim.as_mut().filter(|pending| {
                pending.nonce == claim_nonce && pending.channel == target_channel
            }) else {
                return;
            };
            if accepted {
                if pending.dispatch_reported {
                    return;
                }
                pending.dispatch_reported = true;
            } else if pending.last_failure_detail.as_deref() == Some(detail.as_str()) {
                return;
            } else {
                pending.last_failure_detail = Some(detail.clone());
            }
            let mut state = snapshot.write().expect("snapshot poisoned");
            if accepted {
                add_activity(&mut state, "切换指令已送达", detail, "claim");
            } else {
                // 对端可能只是暂时拿不到接收器（例如刚唤醒或权限服务尚未恢复）。
                // 保留同一个 nonce，让 drive_pending_claim 在短暂间隔后继续重试；
                // 只有真实到达确认或统一超时才结束这次召回。
                add_activity(&mut state, "切换暂未完成，正在重试", detail, "error");
            }
        }
        Payload::ArrivalConfirmed {
            host_id,
            claim_nonce: _,
            channel,
            sent_at_ms,
        } => {
            let mut state = snapshot.write().expect("snapshot poisoned");
            if host_id == state.config.host_id
                || channel != state.config.peer_channel
                || (now_ms() - sent_at_ms).abs() > MESSAGE_MAX_AGE_MS
            {
                return;
            }
            if let Some(source) = udp_source {
                *peer_addr = Some(source);
            }
            state.keyboard.connected = false;
            state.keyboard.current_channel = Some(channel);
            add_activity(
                &mut state,
                "对端已确认键盘上线",
                format!("MX Keys 当前位于通道 {channel}"),
                "success",
            );
        }
    }
}

fn cached_claim_result(
    recent_claims: &VecDeque<RecentClaim>,
    nonce: &str,
) -> Option<(bool, String)> {
    recent_claims
        .iter()
        .find(|claim| claim.nonce == nonce)
        .filter(|claim| claim.accepted || claim.handled_at.elapsed() < CLAIM_DUPLICATE_WINDOW)
        .map(|claim| (claim.accepted, claim.detail.clone()))
}

fn remember_claim_result(
    recent_claims: &mut VecDeque<RecentClaim>,
    nonce: String,
    accepted: bool,
    detail: String,
) {
    recent_claims.retain(|claim| claim.nonce != nonce);
    recent_claims.push_back(RecentClaim {
        nonce,
        handled_at: Instant::now(),
        accepted,
        detail,
    });
    while recent_claims.len() > RECENT_CLAIM_LIMIT {
        recent_claims.pop_front();
    }
}

fn begin_claim(snapshot: &Arc<RwLock<AppSnapshot>>, reason: &str) -> (PendingClaim, Payload) {
    let now = Instant::now();
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut state = snapshot.write().expect("snapshot poisoned");
    state.pending_claim = true;
    let channel = state.config.channel;
    add_activity(
        &mut state,
        reason,
        format!("正在向通道 {channel} 召回键盘"),
        "claim",
    );
    let payload = Payload::Claim {
        host_id: state.config.host_id.clone(),
        channel,
        nonce: nonce.clone(),
        sent_at_ms: now_ms(),
    };
    (
        PendingClaim {
            nonce,
            channel,
            started_at: now,
            last_sent_at: now,
            attempts: 1,
            dispatch_reported: false,
            last_failure_detail: None,
        },
        payload,
    )
}

fn drive_pending_claim(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    pending_claim: &mut Option<PendingClaim>,
) -> Option<Payload> {
    let Some(pending) = pending_claim.as_mut() else {
        return None;
    };
    if pending.started_at.elapsed() >= CLAIM_TIMEOUT || pending.attempts >= CLAIM_MAX_ATTEMPTS {
        let mut state = snapshot.write().expect("snapshot poisoned");
        state.pending_claim = false;
        add_activity(
            &mut state,
            "召回超时",
            "当前持有键盘的电脑没有完成切换；若其正在休眠，请先唤醒".into(),
            "error",
        );
        *pending_claim = None;
        return None;
    }
    if pending.last_sent_at.elapsed() < CLAIM_RETRY_INTERVAL {
        return None;
    }
    if pending.dispatch_reported {
        return None;
    }

    pending.attempts += 1;
    pending.last_sent_at = Instant::now();
    let state = snapshot.read().expect("snapshot poisoned");
    Some(Payload::Claim {
        host_id: state.config.host_id.clone(),
        channel: pending.channel,
        nonce: pending.nonce.clone(),
        sent_at_ms: now_ms(),
    })
}

fn cancel_pending_claim(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    pending_claim: &mut Option<PendingClaim>,
) -> bool {
    if pending_claim.take().is_none() {
        return false;
    }
    snapshot.write().expect("snapshot poisoned").pending_claim = false;
    true
}

fn claim_is_authorized(state: &AppSnapshot, host_id: &str, channel: u8, sent_at_ms: i64) -> bool {
    host_id != state.config.host_id
        && channel == state.config.peer_channel
        && (now_ms() - sent_at_ms).abs() <= MESSAGE_MAX_AGE_MS
}

fn perform_switch(snapshot: &Arc<RwLock<AppSnapshot>>, channel: u8) -> Result<String, String> {
    let started_at = Instant::now();
    match logitech::switch_to(channel) {
        Ok(path) => {
            let elapsed_ms = started_at.elapsed().as_millis();
            let path_label = path.label();
            let mut state = snapshot.write().expect("snapshot poisoned");
            // ChangeHost 成功后键盘会立刻从当前接收器断开，不能等待 HID++ 回复。
            state.keyboard.connected = false;
            state.keyboard.current_channel = Some(channel);
            state.keyboard.supports_change_host = false;
            add_activity(
                &mut state,
                "已发送切换指令",
                format!("MX Keys → 通道 {channel} · {path_label} {elapsed_ms}ms"),
                "success",
            );
            Ok(format!(
                "切换指令已发送 · {path_label} {elapsed_ms}ms，等待目标端确认"
            ))
        }
        Err(error) => {
            let elapsed_ms = started_at.elapsed().as_millis();
            let detail = error.to_string();
            let mut state = snapshot.write().expect("snapshot poisoned");
            state.keyboard.last_error = Some(detail.clone());
            add_activity(
                &mut state,
                "切换失败",
                format!("{detail} · {elapsed_ms}ms"),
                "error",
            );
            Err(format!("{detail} · {elapsed_ms}ms"))
        }
    }
}

fn start_hardware_switch(
    event_sender: &mpsc::UnboundedSender<LocalEvent>,
    snapshot: &Arc<RwLock<AppSnapshot>>,
    channel: u8,
    origin: HardwareSwitchOrigin,
) {
    let event_sender = event_sender.clone();
    let snapshot = snapshot.clone();
    tauri::async_runtime::spawn(async move {
        let result =
            match tokio::task::spawn_blocking(move || perform_switch(&snapshot, channel)).await {
                Ok(result) => result,
                Err(error) => Err(format!("硬件切换任务失败: {error}")),
            };
        let _ = event_sender.send(LocalEvent::HardwareSwitchCompleted(HardwareSwitchOutcome {
            origin,
            channel,
            result,
        }));
    });
}

fn start_hardware_probe(
    event_sender: &mpsc::UnboundedSender<LocalEvent>,
    in_progress: &Arc<AtomicBool>,
) {
    if in_progress.swap(true, Ordering::AcqRel) {
        return;
    }

    let event_sender = event_sender.clone();
    tauri::async_runtime::spawn(async move {
        let hid_access = logitech::hid_access_granted();
        let result = if hid_access {
            match tokio::task::spawn_blocking(logitech::probe).await {
                Ok(Ok(found)) => Ok(found),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(format!("硬件扫描任务失败: {error}")),
            }
        } else {
            Err("请在系统设置 → 隐私与安全性 → 输入监控中允许 KeyClaim".into())
        };
        let _ = event_sender.send(LocalEvent::HardwareProbeCompleted(HardwareProbeOutcome {
            hid_access,
            result,
        }));
    });
}

fn apply_hardware_probe(
    snapshot: &Arc<RwLock<AppSnapshot>>,
    outcome: HardwareProbeOutcome,
) -> Option<u8> {
    if !outcome.hid_access {
        let mut state = snapshot.write().expect("snapshot poisoned");
        state.diagnostics.hid_access = false;
        state.diagnostics.receiver_probe = false;
        state.local_host.ready = false;
        state.keyboard.connected = false;
        state.keyboard.current_channel = None;
        state.keyboard.supports_change_host = false;
        state.keyboard.last_error = outcome.result.err();
        return None;
    }

    let mut state = snapshot.write().expect("snapshot poisoned");
    state.diagnostics.hid_access = true;
    state.diagnostics.receiver_probe = true;
    state.local_host.ready = true;
    match outcome.result {
        Ok(found) => {
            let channel = found.current_channel;
            state.keyboard.connected = true;
            state.keyboard.receiver_name = Some(found.receiver_name);
            state.keyboard.current_channel = Some(found.current_channel);
            state.keyboard.supports_change_host = found.channel_count > 1;
            state.keyboard.last_error = None;
            return Some(channel);
        }
        Err(error) => {
            state.keyboard.connected = false;
            state.keyboard.current_channel = None;
            state.keyboard.supports_change_host = false;
            state.keyboard.last_error = Some(error);
        }
    }
    None
}

fn should_request_claim(state: &AppSnapshot) -> bool {
    state.config.auto_recall
        && state.keyboard.current_channel != Some(state.config.channel)
        && !state.pending_claim
}

fn should_start_tcp_discovery(active_connections: usize) -> bool {
    active_connections == 0
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
    fn offline_peer_cache_does_not_block_keyboard_request() {
        let mut state = mac_snapshot();
        state.peer_host.online = false;
        state.peer_host.ready = false;
        state.keyboard.current_channel = None;

        assert!(should_request_claim(&state));
    }

    #[test]
    fn tcp_discovery_depends_on_connection_instead_of_udp_peer_status() {
        assert!(should_start_tcp_discovery(0));
        assert!(!should_start_tcp_discovery(1));
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

    #[test]
    fn pending_claim_retries_with_same_nonce() {
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));
        let now = Instant::now();
        let mut pending = Some(PendingClaim {
            nonce: "stable-nonce".into(),
            channel: 3,
            started_at: now - Duration::from_millis(800),
            last_sent_at: now - CLAIM_RETRY_INTERVAL,
            attempts: 1,
            dispatch_reported: false,
            last_failure_detail: None,
        });

        let payload = drive_pending_claim(&snapshot, &mut pending).unwrap();
        assert!(matches!(
            payload,
            Payload::Claim {
                nonce,
                channel: 3,
                ..
            } if nonce == "stable-nonce"
        ));
        assert_eq!(pending.unwrap().attempts, 2);
    }

    #[test]
    fn acknowledged_claim_waits_for_arrival_without_resending() {
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));
        let now = Instant::now();
        let mut pending = Some(PendingClaim {
            nonce: "acknowledged".into(),
            channel: 3,
            started_at: now,
            last_sent_at: now - CLAIM_RETRY_INTERVAL,
            attempts: 1,
            dispatch_reported: true,
            last_failure_detail: None,
        });

        assert!(drive_pending_claim(&snapshot, &mut pending).is_none());
        assert_eq!(pending.unwrap().attempts, 1);
    }

    #[test]
    fn newer_remote_intent_releases_the_previous_local_wait() {
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));
        snapshot.write().unwrap().pending_claim = true;
        let now = Instant::now();
        let mut pending = Some(PendingClaim {
            nonce: "local-intent".into(),
            channel: 3,
            started_at: now,
            last_sent_at: now,
            attempts: 1,
            dispatch_reported: false,
            last_failure_detail: None,
        });

        assert!(cancel_pending_claim(&snapshot, &mut pending));
        assert!(pending.is_none());
        assert!(!snapshot.read().unwrap().pending_claim);
    }

    #[test]
    fn concurrent_remote_claims_share_one_hardware_switch() {
        let mut active = ActiveRemoteSwitch::new("first".into(), 2);
        active.coalesce("second".into());
        active.coalesce("second".into());

        assert_eq!(
            active.all_nonces().collect::<Vec<_>>(),
            vec!["first".to_string(), "second".to_string()]
        );
    }

    #[test]
    fn expired_claim_is_cleared_instead_of_reporting_false_success() {
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));
        snapshot.write().unwrap().pending_claim = true;
        let now = Instant::now();
        let mut pending = Some(PendingClaim {
            nonce: "expired".into(),
            channel: 3,
            started_at: now - CLAIM_TIMEOUT,
            last_sent_at: now - CLAIM_RETRY_INTERVAL,
            attempts: 1,
            dispatch_reported: false,
            last_failure_detail: None,
        });

        assert!(drive_pending_claim(&snapshot, &mut pending).is_none());
        assert!(pending.is_none());
        assert!(!snapshot.read().unwrap().pending_claim);
        assert_eq!(snapshot.read().unwrap().activities[0].title, "召回超时");
    }

    #[test]
    fn duplicate_delivery_reuses_recent_failure_without_retouching_hardware() {
        let recent = VecDeque::from([RecentClaim {
            nonce: "same-claim".into(),
            handled_at: Instant::now(),
            accepted: false,
            detail: "暂时无法访问接收器".into(),
        }]);
        assert_eq!(
            cached_claim_result(&recent, "same-claim"),
            Some((false, "暂时无法访问接收器".into()))
        );
    }

    #[test]
    fn failed_claim_can_be_retried_after_duplicate_window() {
        let recent = VecDeque::from([RecentClaim {
            nonce: "retryable".into(),
            handled_at: Instant::now() - CLAIM_DUPLICATE_WINDOW,
            accepted: false,
            detail: "第一次失败".into(),
        }]);
        assert!(cached_claim_result(&recent, "retryable").is_none());
    }

    #[test]
    fn identical_signed_message_is_only_decoded_once() {
        let payload = Payload::Claim {
            host_id: "windows-host".into(),
            channel: 3,
            nonce: "same-frame".into(),
            sent_at_ms: 1,
        };
        let bytes = protocol::encode(payload, "KEYCLAIM").unwrap();
        let mut recent = VecDeque::new();

        assert!(decode_new_payload(&bytes, "KEYCLAIM", &mut recent).is_some());
        assert!(decode_new_payload(&bytes, "KEYCLAIM", &mut recent).is_none());

        let retry = protocol::encode(
            Payload::Claim {
                host_id: "windows-host".into(),
                channel: 3,
                nonce: "same-frame".into(),
                sent_at_ms: 2,
            },
            "KEYCLAIM",
        )
        .unwrap();
        assert!(decode_new_payload(&retry, "KEYCLAIM", &mut recent).is_some());
    }

    #[tokio::test]
    async fn repeated_dispatch_status_is_only_recorded_once() {
        let snapshot = Arc::new(RwLock::new(mac_snapshot()));
        let now = Instant::now();
        let mut pending = Some(PendingClaim {
            nonce: "stable-status".into(),
            channel: 3,
            started_at: now,
            last_sent_at: now,
            attempts: 1,
            dispatch_reported: false,
            last_failure_detail: None,
        });
        let (outgoing, _) = broadcast::channel(4);
        let mut peer_addr = None;
        let mut recent_claims = VecDeque::new();
        let (event_sender, _) = mpsc::unbounded_channel();
        let mut active_remote_switch = None;
        let activity_count = snapshot.read().unwrap().activities.len();

        for sent_at_ms in [now_ms(), now_ms() + 1] {
            handle_network_payload(
                &snapshot,
                Payload::SwitchDispatched {
                    host_id: "windows-host".into(),
                    claim_nonce: "stable-status".into(),
                    target_channel: 3,
                    accepted: true,
                    detail: "切换指令已发送，等待目标端确认".into(),
                    sent_at_ms,
                },
                None,
                &outgoing,
                None,
                &mut peer_addr,
                &mut recent_claims,
                &mut pending,
                &event_sender,
                &mut active_remote_switch,
            )
            .await;
        }

        assert_eq!(
            snapshot.read().unwrap().activities.len(),
            activity_count + 1
        );
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

    #[test]
    fn discovery_keeps_only_the_first_connected_stream() {
        let mut selected = None;
        retain_first(&mut selected, "first");
        retain_first(&mut selected, "second");
        assert_eq!(selected, Some("first"));
    }

    #[tokio::test]
    async fn tcp_outgoing_message_does_not_cancel_partial_incoming_frame() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).await.unwrap();
        let (server, _) = listener.accept().await.unwrap();
        let (outgoing, _) = broadcast::channel(4);
        let (incoming_sender, mut incoming) = mpsc::unbounded_channel();
        let active = Arc::new(AtomicUsize::new(0));

        spawn_tcp_connection(server, outgoing.clone(), incoming_sender, active);

        let incoming_payload = b"complete-frame";
        let length = (incoming_payload.len() as u32).to_be_bytes();
        client.write_all(&length[..2]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;

        outgoing.send(b"heartbeat".to_vec()).unwrap();
        let outgoing_length = client.read_u32().await.unwrap();
        let mut outgoing_payload = vec![0; outgoing_length as usize];
        client.read_exact(&mut outgoing_payload).await.unwrap();
        assert_eq!(outgoing_payload, b"heartbeat");

        client.write_all(&length[2..]).await.unwrap();
        client.write_all(incoming_payload).await.unwrap();
        let received = tokio::time::timeout(Duration::from_secs(1), incoming.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, incoming_payload);
    }

    #[test]
    fn authenticated_peer_claim_does_not_depend_on_stale_runtime_cache() {
        let mut state = mac_snapshot();
        state.peer_host.online = false;
        state.peer_host.ready = false;
        state.keyboard.connected = false;
        state.keyboard.current_channel = None;

        assert!(claim_is_authorized(
            &state,
            "windows-host",
            state.config.peer_channel,
            now_ms(),
        ));
    }

    #[test]
    fn claim_authorization_rejects_self_wrong_channel_and_expired_message() {
        let state = mac_snapshot();
        let current = now_ms();

        assert!(!claim_is_authorized(
            &state,
            &state.config.host_id,
            state.config.peer_channel,
            current,
        ));
        assert!(!claim_is_authorized(
            &state,
            "windows-host",
            state.config.channel,
            current,
        ));
        assert!(!claim_is_authorized(
            &state,
            "windows-host",
            state.config.peer_channel,
            current - MESSAGE_MAX_AGE_MS - 1,
        ));
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
