import { type CSSProperties, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, CircleAlert, LoaderCircle, Settings, X } from "lucide-react";
import "./App.css";
import {
  AppConfig, AppSnapshot, HostView, getChannelHosts, getSnapshot, loadSnapshot, requestHidAccess, saveConfig,
  quitApp,
} from "./api";

const WINDOW_WIDTH = 404;
const WINDOW_HEIGHT = 202;
const ATTENTION_HEIGHT = 256;
const SETTINGS_HEIGHT = 364;

const mockLoading: AppSnapshot = {
  config: { hostId: "loading", hostName: "本机", platform: "macos", channel: 3, peerChannel: 2, pairingCode: "KEYCLAIM", autoRecall: true, launchAtLogin: false, movementThreshold: 3, debounceMs: 60, networkPort: 45823, ignoreSyntheticInput: true },
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
    <label><span>双端配对码</span><input disabled={disabled} value={draft.pairingCode} onChange={(event) => onChange({ ...draft, pairingCode: event.target.value })} /></label>
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

function SettingsDialog({ config, onClose, onSave, onQuit }: { config: AppConfig; onClose: () => void; onSave: (config: AppConfig) => Promise<void>; onQuit: () => void }) {
  const [draft, setDraft] = useState(config);
  const [saving, setSaving] = useState(false);

  const submit = async () => {
    setSaving(true);
    try {
      await onSave(draft);
    } finally {
      setSaving(false);
    }
  };

  return <div className="settings-view">
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-label="KeyClaim · 键来设置">
      <header data-tauri-drag-region>
        <div><span>KeyClaim · 键来</span><h2>设置</h2></div>
        <button className="icon-button" onClick={onClose} aria-label="关闭设置"><X size={18} /></button>
      </header>
      <SettingsFields draft={draft} onChange={setDraft} />
      <footer><button className="quit-button" onClick={onQuit}>退出键来</button><button className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={saving} onClick={submit}>{saving && <LoaderCircle className="spin" size={15} />}保存</button></footer>
    </section>
  </div>;
}

function WindowsMain({ snapshot, channels, loading, message, onQuickChange, onSave }: {
  snapshot: AppSnapshot;
  channels: HostView[];
  loading: boolean;
  message?: string;
  onQuickChange: (patch: Partial<AppConfig>) => void;
  onSave: (config: AppConfig) => Promise<void>;
}) {
  const [draft, setDraft] = useState(snapshot.config);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft((current) => ({
      ...current,
      hostName: snapshot.config.hostName,
      channel: snapshot.config.channel,
      peerChannel: snapshot.config.peerChannel,
      pairingCode: snapshot.config.pairingCode,
      movementThreshold: snapshot.config.movementThreshold,
    }));
  }, [
    snapshot.config.hostName,
    snapshot.config.channel,
    snapshot.config.peerChannel,
    snapshot.config.pairingCode,
    snapshot.config.movementThreshold,
  ]);

  const hasChanges = draft.hostName !== snapshot.config.hostName
    || draft.channel !== snapshot.config.channel
    || draft.peerChannel !== snapshot.config.peerChannel
    || draft.pairingCode !== snapshot.config.pairingCode
    || draft.movementThreshold !== snapshot.config.movementThreshold;

  const submit = async () => {
    setSaving(true);
    try {
      await onSave({ ...snapshot.config, ...draft });
    } finally {
      setSaving(false);
    }
  };

  return <div className="windows-app">
    <header className="windows-header">
      <div className="brand"><strong>KeyClaim</strong><span>键来</span></div>
      <span className="windows-status"><i />{loading ? "正在启动" : "后台运行中"}</span>
    </header>
    <div className="windows-scroll">
      <section className="windows-section" aria-labelledby="channel-heading">
        <div className="section-heading"><h2 id="channel-heading">键盘通道</h2><span>当前通道会自动跟随本机输入</span></div>
        <ChannelStrip snapshot={snapshot} channels={channels} className="windows-channel-strip" />
      </section>
      <QuickControls config={snapshot.config} loading={loading} onChange={onQuickChange} className="windows-quick-controls" />
      {message && <div className="windows-message" role="status"><CircleAlert size={15} /><span>{message}</span></div>}
      <section className="windows-settings" aria-labelledby="settings-heading">
        <div className="section-heading"><h2 id="settings-heading">设备设置</h2></div>
        <SettingsFields draft={draft} onChange={setDraft} disabled={loading} />
      </section>
    </div>
    <footer className="windows-footer">
      <span>{hasChanges ? "有未保存的更改" : "设置已保存"}</span>
      <button className="primary-button" disabled={loading || !hasChanges || saving} onClick={submit}>{saving && <LoaderCircle className="spin" size={15} />}保存更改</button>
    </footer>
  </div>;
}

function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(mockLoading);
  const [loading, setLoading] = useState(true);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [message, setMessage] = useState<string>();

  useEffect(() => {
    let mounted = true;
    loadSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => { if (mounted) setMessage("读取本机状态失败"); }).finally(() => { if (mounted) setLoading(false); });
    const timer = window.setInterval(() => getSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => undefined), 900);
    return () => { mounted = false; window.clearInterval(timer); };
  }, []);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (snapshot.config.platform !== "windows" && event.key === "," && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setSettingsOpen(true); }
      if (snapshot.config.platform !== "windows" && event.key === "Escape") setSettingsOpen(false);
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [snapshot.config.platform]);

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
    if (!("__TAURI_INTERNALS__" in window) || loading || snapshot.config.platform === "windows") return;
    const height = settingsOpen ? SETTINGS_HEIGHT : attentionMessage ? ATTENTION_HEIGHT : WINDOW_HEIGHT;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow, LogicalSize }) => getCurrentWindow().setSize(new LogicalSize(WINDOW_WIDTH, height))).catch(() => undefined);
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

  if (isWindowsRuntime) {
    return <WindowsMain key={loading ? "loading" : "ready"} snapshot={snapshot} channels={channels} loading={loading} message={message} onQuickChange={updateQuickSetting} onSave={save} />;
  }

  if (settingsOpen) {
    return <SettingsDialog config={snapshot.config} onClose={() => setSettingsOpen(false)} onSave={async (config) => { await save(config); setSettingsOpen(false); }} onQuit={() => { void quitApp(); }} />;
  }

  return <div className={`app-shell ${attentionMessage ? "has-attention" : ""}`}>
    <header className="app-header" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><strong>KeyClaim</strong><span>键来</span></div>
      <button className="icon-button" onClick={() => setSettingsOpen(true)} aria-label="打开设置" title="设置（⌘,）"><Settings size={18} /></button>
    </header>

    <main>
      <ChannelStrip snapshot={snapshot} channels={channels} />

      {attentionMessage && <div className="attention-row" role="status"><CircleAlert size={16} /><span>{attentionMessage}</span>{needsHidPermission && <button onClick={handlePermission}>前往允许</button>}</div>}

      <QuickControls config={snapshot.config} loading={loading} onChange={updateQuickSetting} />
    </main>
  </div>;
}

export default App;
