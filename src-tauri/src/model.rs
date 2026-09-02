use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Macos
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub host_id: String,
    pub host_name: String,
    pub platform: Platform,
    pub channel: u8,
    pub peer_channel: u8,
    pub pairing_code: String,
    pub auto_recall: bool,
    pub launch_at_login: bool,
    pub movement_threshold: i32,
    pub debounce_ms: u64,
    pub network_port: u16,
    pub ignore_synthetic_input: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let platform = Platform::current();
        let (channel, peer_channel) = match platform {
            Platform::Macos => (3, 2),
            Platform::Windows => (2, 3),
        };
        Self {
            host_id: uuid::Uuid::new_v4().to_string(),
            host_name: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            platform,
            channel,
            peer_channel,
            pairing_code: "KEYCLAIM".into(),
            auto_recall: true,
            launch_at_login: false,
            movement_threshold: 3,
            debounce_ms: 60,
            network_port: 45_823,
            ignore_synthetic_input: true,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostView {
    pub name: String,
    pub platform: Platform,
    pub channel: u8,
    pub online: bool,
    pub ready: bool,
    pub last_seen_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardView {
    pub connected: bool,
    pub receiver_name: Option<String>,
    pub current_channel: Option<u8>,
    pub supports_change_host: bool,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: String,
    pub timestamp: String,
    pub title: String,
    pub detail: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    pub input_monitor: bool,
    pub network_listener: bool,
    pub receiver_probe: bool,
    pub synthetic_filter_mode: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub config: AppConfig,
    pub local_host: HostView,
    pub peer_host: HostView,
    pub keyboard: KeyboardView,
    pub diagnostics: DiagnosticsView,
    pub activities: Vec<ActivityEntry>,
    pub last_local_input_ms: Option<i64>,
    pub pending_claim: bool,
}

impl AppSnapshot {
    pub fn initial(config: AppConfig) -> Self {
        Self {
            local_host: HostView {
                name: config.host_name.clone(),
                platform: config.platform.clone(),
                channel: config.channel,
                online: true,
                ready: true,
                last_seen_ms: Some(chrono::Utc::now().timestamp_millis()),
            },
            peer_host: HostView {
                name: match config.platform {
                    Platform::Macos => "Windows 工作站".into(),
                    Platform::Windows => "MacBook Pro".into(),
                },
                platform: match config.platform {
                    Platform::Macos => Platform::Windows,
                    Platform::Windows => Platform::Macos,
                },
                channel: config.peer_channel,
                online: false,
                ready: false,
                last_seen_ms: None,
            },
            keyboard: KeyboardView {
                connected: false,
                receiver_name: None,
                current_channel: None,
                supports_change_host: false,
                last_error: None,
            },
            diagnostics: DiagnosticsView {
                input_monitor: false,
                network_listener: false,
                receiver_probe: false,
                synthetic_filter_mode: "实验性：当前按光标位移识别".into(),
            },
            activities: vec![],
            last_local_input_ms: None,
            pending_claim: false,
            config,
        }
    }
}
