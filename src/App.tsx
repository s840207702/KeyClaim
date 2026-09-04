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

  const changeChannel = (channel: number) => {
    const peerChannel = draft.peerChannel === channel
      ? ([1, 2, 3].find((candidate) => candidate !== channel) ?? draft.peerChannel)
      : draft.peerChannel;
    setDraft({ ...draft, channel, peerChannel });
  };

  return <div className="settings-view">
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-label="KeyClaim · 键来设置">
      <header data-tauri-drag-region>
        <div><span>KeyClaim · 键来</span><h2>设置</h2></div>
        <button className="icon-button" onClick={onClose} aria-label="关闭设置"><X size={18} /></button>
      </header>
      <div className="settings-form">
        <label><span>本机名称</span><input value={draft.hostName} onChange={(event) => setDraft({ ...draft, hostName: event.target.value })} /></label>
        <label><span>本机通道</span><select value={draft.channel} onChange={(event) => changeChannel(Number(event.target.value))}><option value={1}>通道 1</option><option value={2}>通道 2</option><option value={3}>通道 3</option></select></label>
        <label><span>双端配对码</span><input value={draft.pairingCode} onChange={(event) => setDraft({ ...draft, pairingCode: event.target.value })} /></label>
        <label><span>指针灵敏度</span><select value={draft.movementThreshold} onChange={(event) => setDraft({ ...draft, movementThreshold: Number(event.target.value) })}><option value={2}>灵敏</option><option value={3}>推荐</option><option value={6}>稳健</option></select></label>
      </div>
      <footer><button className="quit-button" onClick={onQuit}>退出键来</button><button className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={saving} onClick={submit}>{saving && <LoaderCircle className="spin" size={15} />}保存</button></footer>
    </section>
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
      if (event.key === "," && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setSettingsOpen(true); }
      if (event.key === "Escape") setSettingsOpen(false);
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let unlisten: (() => void) | undefined;
    void listen("popover-hidden", () => setSettingsOpen(false)).then((stopListening) => { unlisten = stopListening; }).catch(() => undefined);
    return () => unlisten?.();
  }, []);

  const channels = useMemo(() => {
    return getChannelHosts(snapshot);
  }, [snapshot]);

  const needsHidPermission = !loading && snapshot.config.platform === "macos" && !snapshot.diagnostics.hidAccess;
  const attentionMessage = message ?? (needsHidPermission ? "需要允许设备控制，键来才能访问键盘接收器。" : undefined);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const height = settingsOpen ? SETTINGS_HEIGHT : attentionMessage ? ATTENTION_HEIGHT : WINDOW_HEIGHT;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow, LogicalSize }) => getCurrentWindow().setSize(new LogicalSize(WINDOW_WIDTH, height))).catch(() => undefined);
  }, [attentionMessage, settingsOpen]);

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
    return <SettingsDialog config={snapshot.config} onClose={() => setSettingsOpen(false)} onSave={async (config) => { await save(config); setSettingsOpen(false); }} onQuit={() => { void quitApp(); }} />;
  }

  return <div className={`app-shell ${attentionMessage ? "has-attention" : ""}`}>
    <header className="app-header" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><strong>KeyClaim</strong><span>键来</span></div>
      <button className="icon-button" onClick={() => setSettingsOpen(true)} aria-label="打开设置" title="设置（⌘,）"><Settings size={18} /></button>
    </header>

    <main>
      <section className="channel-strip" aria-label="键盘通道" style={{ "--channel-count": channels.length } as CSSProperties}>
        {channels.map((host) => {
          const active = snapshot.keyboard.currentChannel === host.channel;
          return <article className={`channel ${active ? "is-active" : ""}`} key={host.channel} aria-current={active ? "true" : undefined} title={`${host.name} · 通道 ${host.channel}`}>
            <span>通道 {host.channel}</span>
            <strong>{friendlyHostName(host)}</strong>
            {active && <span className="active-mark" aria-label="当前通道">{snapshot.pendingClaim ? <LoaderCircle className="spin" size={13} /> : <Check size={13} />}</span>}
          </article>;
        })}
      </section>

      {attentionMessage && <div className="attention-row" role="status"><CircleAlert size={16} /><span>{attentionMessage}</span>{needsHidPermission && <button onClick={handlePermission}>前往允许</button>}</div>}

      <section className="quick-controls" aria-label="常用设置">
        <div className="quick-control"><span>自动跟随</span><Toggle checked={snapshot.config.autoRecall} disabled={loading} onChange={(autoRecall) => updateQuickSetting({ autoRecall })} label="自动跟随" /></div>
        <div className="quick-control"><span>开机自启</span><Toggle checked={snapshot.config.launchAtLogin} disabled={loading} onChange={(launchAtLogin) => updateQuickSetting({ launchAtLogin })} label="开机自启" /></div>
      </section>
    </main>
  </div>;
}

export default App;
