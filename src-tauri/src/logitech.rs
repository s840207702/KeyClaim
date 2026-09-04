#[cfg(not(target_os = "macos"))]
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::sync::{Mutex, OnceLock};
#[cfg(not(target_os = "macos"))]
use std::time::{Duration, Instant};

#[cfg(not(target_os = "macos"))]
const LOGITECH_VID: u16 = 0x046d;
const UNIFYING_PID: u16 = 0xc52b;
#[cfg(not(target_os = "macos"))]
const FEATURE_ROOT: u8 = 0x00;
#[cfg(not(target_os = "macos"))]
const FEATURE_DEVICE_TYPE_AND_NAME: u16 = 0x0005;
#[cfg(not(target_os = "macos"))]
const FEATURE_CHANGE_HOST: u16 = 0x1814;
#[cfg(not(target_os = "macos"))]
const DEVICE_TYPE_KEYBOARD: u8 = 0;
#[cfg(not(target_os = "macos"))]
const SOFTWARE_ID: u8 = 0x0a;
#[cfg(not(target_os = "macos"))]
const REPORT_SHORT: u8 = 0x10;
#[cfg(not(target_os = "macos"))]
const REPORT_LONG: u8 = 0x11;
#[cfg(not(target_os = "macos"))]
const DEVICE_INDICES: [u8; 7] = [0xff, 1, 2, 3, 4, 5, 6];

#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub receiver_name: String,
    pub current_channel: u8,
    pub channel_count: u8,
    #[cfg(not(target_os = "macos"))]
    path: Vec<u8>,
    #[cfg(not(target_os = "macos"))]
    device_index: u8,
    #[cfg(not(target_os = "macos"))]
    report_id: u8,
    #[cfg(not(target_os = "macos"))]
    change_host_index: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchPath {
    Cached,
    Discovered,
}

impl SwitchPath {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cached => "热路径",
            Self::Discovered => "冷路径",
        }
    }
}

static DEVICE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn device_io_lock() -> &'static Mutex<()> {
    DEVICE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

#[cfg(not(target_os = "macos"))]
static SWITCH_CACHE: OnceLock<Mutex<Option<ProbeResult>>> = OnceLock::new();

#[cfg(not(target_os = "macos"))]
fn switch_cache() -> &'static Mutex<Option<ProbeResult>> {
    SWITCH_CACHE.get_or_init(|| Mutex::new(None))
}

#[derive(Debug, thiserror::Error)]
pub enum LogitechError {
    #[cfg(not(target_os = "macos"))]
    #[error("无法初始化 HID: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("未找到支持 Easy-Switch 的 MX Keys；请先轻按键盘唤醒")]
    NotFound,
    #[cfg(target_os = "macos")]
    #[error("macOS 尚未允许 KeyClaim 使用输入监控，无法访问 Logitech 接收器")]
    PermissionDenied,
    #[error("通道必须是 1、2 或 3")]
    InvalidChannel,
    #[cfg(not(target_os = "macos"))]
    #[error("接收器响应超时")]
    Timeout,
}

#[cfg(not(target_os = "macos"))]
fn frame_len(report_id: u8) -> usize {
    if report_id == REPORT_SHORT { 7 } else { 20 }
}

#[cfg(not(target_os = "macos"))]
fn request(
    device: &HidDevice,
    report_id: u8,
    device_index: u8,
    feature_index: u8,
    function: u8,
    params: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, LogitechError> {
    let mut frame = vec![0; frame_len(report_id)];
    frame[0] = report_id;
    frame[1] = device_index;
    frame[2] = feature_index;
    frame[3] = (function << 4) | SOFTWARE_ID;
    let count = params.len().min(frame.len() - 4);
    frame[4..4 + count].copy_from_slice(&params[..count]);
    device.write(&frame)?;

    let deadline = Instant::now() + timeout;
    let mut reply = [0u8; 32];
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait_ms = remaining.as_millis().min(100).max(1) as i32;
        let size = device.read_timeout(&mut reply, wait_ms)?;
        if size < 5 || reply[1] != device_index {
            continue;
        }
        if reply[2] == 0xff || reply[2] == 0x8f {
            continue;
        }
        if reply[2] == feature_index && reply[3] & 0x0f == SOFTWARE_ID {
            return Ok(reply[..size].to_vec());
        }
    }
    Err(LogitechError::Timeout)
}

#[cfg(not(target_os = "macos"))]
fn resolve_feature(
    device: &HidDevice,
    report_id: u8,
    device_index: u8,
    feature: u16,
) -> Result<u8, LogitechError> {
    let params = [(feature >> 8) as u8, feature as u8, 0];
    let reply = request(
        device,
        report_id,
        device_index,
        FEATURE_ROOT,
        0,
        &params,
        Duration::from_millis(220),
    )?;
    reply
        .get(4)
        .copied()
        .filter(|index| *index != 0)
        .ok_or(LogitechError::NotFound)
}

#[cfg(not(target_os = "macos"))]
fn is_keyboard(device: &HidDevice, report_id: u8, device_index: u8) -> Result<bool, LogitechError> {
    let index = resolve_feature(
        device,
        report_id,
        device_index,
        FEATURE_DEVICE_TYPE_AND_NAME,
    )?;
    let reply = request(
        device,
        report_id,
        device_index,
        index,
        2,
        &[],
        Duration::from_millis(220),
    )?;
    Ok(reply.get(4).copied() == Some(DEVICE_TYPE_KEYBOARD))
}

#[cfg(not(target_os = "macos"))]
fn candidate_rank(info: &DeviceInfo) -> u8 {
    if info.product_id() == UNIFYING_PID {
        0
    } else {
        1
    }
}

#[cfg(not(target_os = "macos"))]
fn probe_hidapi() -> Result<ProbeResult, LogitechError> {
    let api = HidApi::new()?;
    let mut candidates: Vec<DeviceInfo> = api
        .device_list()
        .filter(|info| info.vendor_id() == LOGITECH_VID)
        .filter(|info| info.usage_page() >= 0xff00)
        .cloned()
        .collect();
    candidates.sort_by_key(candidate_rank);

    for info in candidates {
        let Ok(device) = info.open_device(&api) else {
            continue;
        };
        for device_index in DEVICE_INDICES {
            for report_id in [REPORT_LONG, REPORT_SHORT] {
                if !matches!(is_keyboard(&device, report_id, device_index), Ok(true)) {
                    continue;
                }
                let Ok(change_host_index) =
                    resolve_feature(&device, report_id, device_index, FEATURE_CHANGE_HOST)
                else {
                    continue;
                };
                let Ok(host) = request(
                    &device,
                    report_id,
                    device_index,
                    change_host_index,
                    0,
                    &[],
                    Duration::from_millis(300),
                ) else {
                    continue;
                };
                if host.len() < 6 {
                    continue;
                }
                return Ok(ProbeResult {
                    receiver_name: info
                        .product_string()
                        .unwrap_or("Logitech 接收器")
                        .to_string(),
                    current_channel: host[5].saturating_add(1),
                    channel_count: host[4],
                    path: info.path().to_bytes().to_vec(),
                    device_index,
                    report_id,
                    change_host_index,
                });
            }
        }
    }
    Err(LogitechError::NotFound)
}

#[cfg(not(target_os = "macos"))]
fn probe_cached_hidapi(found: &ProbeResult) -> Result<ProbeResult, LogitechError> {
    let api = HidApi::new()?;
    let path = std::ffi::CString::new(found.path.clone()).map_err(|_| LogitechError::NotFound)?;
    let device = api.open_path(&path)?;
    let host = request(
        &device,
        found.report_id,
        found.device_index,
        found.change_host_index,
        0,
        &[],
        Duration::from_millis(120),
    )?;
    if host.len() < 6 {
        return Err(LogitechError::NotFound);
    }
    let mut refreshed = found.clone();
    refreshed.channel_count = host[4];
    refreshed.current_channel = host[5].saturating_add(1);
    Ok(refreshed)
}

#[cfg(not(target_os = "macos"))]
fn write_switch_hidapi(found: &ProbeResult, channel: u8) -> Result<(), LogitechError> {
    let api = HidApi::new()?;
    let path = std::ffi::CString::new(found.path.clone()).map_err(|_| LogitechError::NotFound)?;
    let device = api.open_path(&path)?;
    let mut frame = vec![0; frame_len(found.report_id)];
    frame[0] = found.report_id;
    frame[1] = found.device_index;
    frame[2] = found.change_host_index;
    frame[3] = (1 << 4) | SOFTWARE_ID;
    frame[4] = channel - 1;
    device.write(&frame)?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn switch_hidapi(channel: u8) -> Result<SwitchPath, LogitechError> {
    if !(1..=3).contains(&channel) {
        return Err(LogitechError::InvalidChannel);
    }

    let cached = switch_cache()
        .lock()
        .expect("switch cache poisoned")
        .clone();
    if let Some(found) = cached {
        if write_switch_hidapi(&found, channel).is_ok() {
            // Easy-Switch 不会改变接收器路径、配对设备索引或功能索引。
            // 保留路由，键盘快速返回时可以继续走单次写入的热路径。
            return Ok(SwitchPath::Cached);
        }
        *switch_cache().lock().expect("switch cache poisoned") = None;
    }

    let found = probe_hidapi()?;
    write_switch_hidapi(&found, channel)?;
    *switch_cache().lock().expect("switch cache poisoned") = Some(found);
    Ok(SwitchPath::Discovered)
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct NativeProbeResult {
    current_channel: i32,
    channel_count: i32,
    device_index: i32,
    report_id: i32,
    feature_index: i32,
    product_id: i32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn keyclaim_probe_keyboard(output: *mut NativeProbeResult) -> i32;
    fn keyclaim_switch_keyboard(channel: i32) -> i32;
    fn keyclaim_hid_access_status() -> i32;
    fn keyclaim_request_hid_access() -> i32;
}

#[cfg(target_os = "macos")]
pub fn hid_access_granted() -> bool {
    // SAFETY: 原生函数不接收指针，只返回系统当前的只读授权状态。
    unsafe { keyclaim_hid_access_status() == 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn hid_access_granted() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub fn request_hid_access() -> bool {
    // SAFETY: 原生函数仅调用 macOS 官方授权接口，不读写调用方内存。
    unsafe { keyclaim_request_hid_access() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn request_hid_access() -> bool {
    true
}

#[cfg(target_os = "macos")]
pub fn probe() -> Result<ProbeResult, LogitechError> {
    let _device_guard = device_io_lock().lock().expect("device io lock poisoned");
    if !hid_access_granted() {
        return Err(LogitechError::PermissionDenied);
    }
    let mut native = NativeProbeResult::default();
    // SAFETY: C 端只在调用期间写入该定长结构体，不保留指针。
    let success = unsafe { keyclaim_probe_keyboard(&mut native) };
    if success == 0 {
        return Err(LogitechError::NotFound);
    }
    Ok(ProbeResult {
        receiver_name: if native.product_id == UNIFYING_PID as i32 {
            "Logitech Unifying Receiver".into()
        } else {
            "Logitech USB Receiver".into()
        },
        current_channel: native.current_channel as u8,
        channel_count: native.channel_count as u8,
    })
}

#[cfg(not(target_os = "macos"))]
pub fn probe() -> Result<ProbeResult, LogitechError> {
    let _device_guard = device_io_lock().lock().expect("device io lock poisoned");
    let cached = switch_cache()
        .lock()
        .expect("switch cache poisoned")
        .clone();
    let result = match cached.as_ref() {
        Some(found) => probe_cached_hidapi(found),
        None => probe_hidapi(),
    };
    match &result {
        Ok(found) => {
            *switch_cache().lock().expect("switch cache poisoned") = Some(found.clone());
        }
        // 已知路由短暂读不到键盘，通常只是键盘仍在另一通道。此时保留缓存，
        // 避免 500ms 后台探测退化为数秒全量扫描并挡住下一次 ChangeHost。
        Err(_) if cached.is_none() => {
            *switch_cache().lock().expect("switch cache poisoned") = None;
        }
        Err(_) => {}
    }
    result
}

#[cfg(target_os = "macos")]
pub fn switch_to(channel: u8) -> Result<SwitchPath, LogitechError> {
    if !(1..=3).contains(&channel) {
        return Err(LogitechError::InvalidChannel);
    }
    if !hid_access_granted() {
        return Err(LogitechError::PermissionDenied);
    }
    let _device_guard = device_io_lock().lock().expect("device io lock poisoned");
    // SAFETY: C 端只接收整数值，目标通道已在上方校验；2 表示命中缓存。
    match unsafe { keyclaim_switch_keyboard(channel as i32) } {
        2 => Ok(SwitchPath::Cached),
        1 => Ok(SwitchPath::Discovered),
        _ => Err(LogitechError::NotFound),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn switch_to(channel: u8) -> Result<SwitchPath, LogitechError> {
    let _device_guard = device_io_lock().lock().expect("device io lock poisoned");
    switch_hidapi(channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_channel_fails_without_touching_hardware() {
        assert!(matches!(switch_to(0), Err(LogitechError::InvalidChannel)));
        assert!(matches!(switch_to(4), Err(LogitechError::InvalidChannel)));
    }

    #[test]
    #[ignore = "只读真实 Logitech 接收器，按需手动运行"]
    fn probe_connected_keyboard() {
        println!("{:#?}", probe());
    }
}
