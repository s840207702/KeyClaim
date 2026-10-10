import { type CSSProperties, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, CircleAlert, LoaderCircle, Settings, X } from "lucide-react";
import "./App.css";
import {
  AppConfig, AppSnapshot, HostView, getChannelHosts, getSnapshot, loadSnapshot, requestHidAccess, saveConfig,
  CURRENT_VERSION, getAppVersion, quitApp,
} from "./api";

const WINDOW_WIDTH = 404;
const WINDOW_HEIGHT = 202;
const ATTENTION_HEIGHT = 256;
const SETTINGS_HEIGHT = 364;
const WINDOWS_SETTINGS_HEIGHT = 380;

const mockLoading: AppSnapshot = {
  config: { hostId: "loading", hostName: "本机", platform: "macos", channel: 3, peerChannel: 2, pairingCode: "", autoRecall: true, launchAtLogin: false, movementThreshold: 3, debounceMs: 60, networkPort: 45823, ignoreSyntheticInput: true },
  localHost: { name: "MacBook Pro", platform: "macos", channel: 3, online: true, ready: false },
  peerHost: { name: "Windows 工作站", platform: "windows", channel: 2, online: false, ready: false },
  keyboard: { connected: false, supportsChangeHost: false },
  diagnostics: { inputMonitor: false, networkListener: false, hidAccess: false, receiverProbe: false, syntheticFilterMode: "正在初始化" },
  activities: [],
  pendingClaim: false,
};

function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" className={`toggle ${checked ? "is-on" : ""}`} role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>;
}

function Brand({ version }: { version: string }) {
  return <div className="brand"><strong>KeyClaim</strong><span>键来</span><small>v{version}</small></div>;
}

function friendlyHostName(host: HostView) {
  const normalized = host.name.replace(/\.local$/i, "").replace(/-/g, " ");
  if (/macbook\s*pro/i.test(normalized)) return "MacBook Pro";
  if (/macbook/i.test(normalized)) return "MacBook";
  return normalized;
}

function changeChannel(config: AppConfig, channel: number) {
  const peerChannel = config.peerChannel === channel
    ? ([1, 2, 3].find((candidate) => candidate !== channel) ?? config.peerChannel)
    : config.peerChannel;
  return { ...config, channel, peerChannel };
}

function SettingsFields({ draft, onChange, disabled = false }: { draft: AppConfig; onChange: (config: AppConfig) => void; disabled?: boolean }) {
  return <div className="settings-fields">
    <label><span>本机名称</span><input disabled={disabled} value={draft.hostName} onChange={(event) => onChange({ ...draft, hostName: event.target.value })} /></label>
    <label><span>本机通道</span><select disabled={disabled} value={draft.channel} onChange={(event) => onChange(changeChannel(draft, Number(event.target.value)))}><option value={1}>通道 1</option><option value={2}>通道 2</option><option value={3}>通道 3</option></select></label>
    <label><span>双端配对码</span><input type={draft.platform === "windows" ? "password" : "text"} autoComplete="off" disabled={disabled} value={draft.pairingCode} onChange={(event) => onChange({ ...draft, pairingCode: event.target.value })} /></label>
    <label><span>指针灵敏度</span><select disabled={disabled} value={draft.movementThreshold} onChange={(event) => onChange({ ...draft, movementThreshold: Number(event.target.value) })}><option value={2}>灵敏</option><option value={3}>推荐</option><option value={6}>稳健</option></select></label>
  </div>;
}

function ChannelStrip({ snapshot, channels, className = "" }: { snapshot: AppSnapshot; channels: HostView[]; className?: string }) {
  return <section className={`channel-strip ${className}`} aria-label="键盘通道" style={{ "--channel-count": channels.length } as CSSProperties}>
    {channels.map((host) => {
      const active = snapshot.keyboard.currentChannel === host.channel;
      return <article className={`channel ${active ? "is-active" : ""}`} key={host.channel} aria-current={active ? "true" : undefined} title={`${host.name} · 通道 ${host.channel}`}>
        <span>通道 {host.channel}</span>
        <strong>{friendlyHostName(host)}</strong>
        {active && <span className="active-mark" aria-label="当前通道">{snapshot.pendingClaim ? <LoaderCircle className="spin" size={13} /> : <Check size={13} />}</span>}
      </article>;
    })}
  </section>;
}

function QuickControls({ config, loading, onChange, className = "" }: { config: AppConfig; loading: boolean; onChange: (patch: Partial<AppConfig>) => void; className?: string }) {
  return <section className={`quick-controls ${className}`} aria-label="常用设置">
    <div className="quick-control"><span>自动跟随</span><Toggle checked={config.autoRecall} disabled={loading} onChange={(autoRecall) => onChange({ autoRecall })} label="自动跟随" /></div>
    <div className="quick-control"><span>开机自启</span><Toggle checked={config.launchAtLogin} disabled={loading} onChange={(launchAtLogin) => onChange({ launchAtLogin })} label="开机自启" /></div>
  </section>;
}

function SettingsDialog({ config, version, message, onClose, onSave, onQuit }: { config: AppConfig; version: string; message?: string; onClose: () => void; onSave: (config: AppConfig) => Promise<void>; onQuit: () => void }) {
  const [draft, setDraft] = useState(config);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); if (!saving) onClose(); }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [onClose, saving]);

  const submit = async () => {
    setSaving(true);
    try {
      await onSave(draft);
    } catch {
      // App 将保存失败的信息显示在设置面板中，保留输入以便重试。
    } finally {
      setSaving(false);
    }
  };

  return <div className={`settings-view ${config.platform === "windows" ? "windows-settings-view" : ""}`}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-label="KeyClaim · 键来设置">
      <header data-tauri-drag-region>
        <div><span>KeyClaim · 键来 · v{version}</span><h2>设置</h2></div>
        <button className="icon-button" disabled={saving} onClick={onClose} aria-label="关闭设置"><X size={18} /></button>
      </header>
      <SettingsFields draft={draft} onChange={setDraft} disabled={saving} />
      {message && <div className="attention-row settings-message" role="status"><CircleAlert size={16} /><span>{message}</span></div>}
      <footer><button className="quit-button" disabled={saving} onClick={onQuit}>退出键来</button><button className="secondary-button" disabled={saving} onClick={onClose}>取消</button><button className="primary-button" disabled={saving} onClick={submit}>{saving && <LoaderCircle className="spin" size={15} />}保存</button></footer>
    </section>
  </div>;
}


function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(mockLoading);
  const [loading, setLoading] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [message, setMessage] = useState<string>();
  const [version, setVersion] = useState(CURRENT_VERSION);
  const windowsHomeHeight = useRef<number | null>(null);

  useEffect(() => {
    let mounted = true;
    loadSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => { if (mounted) setMessage("读取本机状态失败"); }).finally(() => { if (mounted) setLoading(false); });
    const timer = window.setInterval(() => getSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => undefined), 900);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  useEffect(() => {
    void getAppVersion().then(setVersion).catch(() => undefined);
  }, []);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (loading) return;
      if (event.key === "," && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setSettingsOpen(true); }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [loading]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void listen("popover-hidden", () => setSettingsOpen(false)).then((stopListening) => { unlisten = stopListening; }).catch(() => undefined);
    return () => unlisten?.();
  }, []);

  const channels = useMemo(() => {
    return getChannelHosts(snapshot);
  }, [snapshot]);
  const isWindowsRuntime = snapshot.config.platform === "windows" || /Windows/i.test(navigator.userAgent);

  const needsHidPermission = !loading && snapshot.config.platform === "macos" && !snapshot.diagnostics.hidAccess;
  const attentionMessage = message ?? (needsHidPermission ? "需要允许设备控制，键来才能访问键盘接收器。" : undefined);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window) || loading) return;
    let cancelled = false;
    void import("@tauri-apps/api/window").then(async ({ getCurrentWindow, LogicalSize }) => {
      const appWindow = getCurrentWindow();
      if (snapshot.config.platform === "windows") {
        const [size, scale] = await Promise.all([appWindow.innerSize(), appWindow.scaleFactor()]);
        if (cancelled) return;
        const logicalSize = size.toLogical(scale);
        if (settingsOpen) {
          windowsHomeHeight.current ??= logicalSize.height;
          await appWindow.setSize(new LogicalSize(logicalSize.width, Math.max(WINDOWS_SETTINGS_HEIGHT, logicalSize.height)));
        } else if (windowsHomeHeight.current !== null) {
          const height = windowsHomeHeight.current;
          windowsHomeHeight.current = null;
          await appWindow.setSize(new LogicalSize(logicalSize.width, height));
        }
      } else if (!cancelled) {
        const height = settingsOpen ? SETTINGS_HEIGHT : attentionMessage ? ATTENTION_HEIGHT : WINDOW_HEIGHT;
        await appWindow.setSize(new LogicalSize(WINDOW_WIDTH, height));
      }
    }).catch(() => undefined);
    return () => { cancelled = true; };
  }, [attentionMessage, loading, settingsOpen, snapshot.config.platform]);

  const save = async (config: AppConfig) => {
    try {
      setSnapshot(await saveConfig(config));
      setMessage(undefined);
    } catch {
      setMessage("设置没有保存成功，请重试");
      throw new Error("save failed");
    }
  };

  const updateQuickSetting = async (patch: Partial<AppConfig>) => {
    try {
      await save({ ...snapshot.config, ...patch });
    } catch {
      // 可见错误由 save 统一处理。
    }
  };

  const handlePermission = async () => {
    setMessage(undefined);
    try {
      setSnapshot(await requestHidAccess());
    } catch {
      setMessage("没有成功发起设备控制授权，请在系统设置中允许 KeyClaim · 键来");
    }
  };

  if (settingsOpen) {
    return <SettingsDialog config={snapshot.config} version={version} message={message} onClose={() => setSettingsOpen(false)} onSave={async (config) => {
      await save({ ...snapshot.config, hostName: config.hostName, channel: config.channel, peerChannel: config.peerChannel, pairingCode: config.pairingCode, movementThreshold: config.movementThreshold });
      setSettingsOpen(false);
    }} onQuit={() => { void quitApp(); }} />;
  }

  return <div className={`app-shell ${isWindowsRuntime ? "windows-app" : ""} ${attentionMessage ? "has-attention" : ""}`}>
    <header className="app-header" data-tauri-drag-region>
      <div data-tauri-drag-region><Brand version={version} /></div>
      <button className="icon-button" disabled={loading} onClick={() => setSettingsOpen(true)} aria-label="打开设置" title={isWindowsRuntime ? "设置（Ctrl+,）" : "设置（⌘,）"}><Settings size={18} /></button>
    </header>

    <main>
      <ChannelStrip snapshot={snapshot} channels={channels} />

      {attentionMessage && <div className="attention-row" role="status"><CircleAlert size={16} /><span>{attentionMessage}</span>{needsHidPermission && <button onClick={handlePermission}>前往允许</button>}</div>}

      <QuickControls config={snapshot.config} loading={loading} onChange={updateQuickSetting} />
    </main>
  </div>;
}

export default App;
