import { useEffect, useMemo, useState } from "react";
import {
  Check, CircleAlert, Keyboard, Laptop, LoaderCircle, Monitor,
  MousePointer2, RefreshCw, Settings, ShieldCheck, Wifi, X,
} from "lucide-react";
import "./App.css";
import {
  AppConfig, AppSnapshot, getAppVersion, getSnapshot, loadSnapshot, refreshHardware, saveConfig,
  switchKeyboard,
} from "./api";

const mockLoading: AppSnapshot = {
  config: { hostId: "loading", hostName: "本机", platform: "macos", channel: 3, peerChannel: 2, pairingCode: "KEYCLAIM", autoRecall: true, launchAtLogin: false, movementThreshold: 3, debounceMs: 60, networkPort: 45823, ignoreSyntheticInput: true },
  localHost: { name: "MacBook Pro", platform: "macos", channel: 3, online: true, ready: false },
  peerHost: { name: "Windows 工作站", platform: "windows", channel: 2, online: false, ready: false },
  keyboard: { connected: false, supportsChangeHost: false },
  diagnostics: { inputMonitor: false, networkListener: false, receiverProbe: false, syntheticFilterMode: "正在初始化" },
  activities: [],
  pendingClaim: false,
};

function Toggle({ checked, onChange, label, disabled = false }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <button type="button" className={`toggle ${checked ? "is-on" : ""}`} role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)}><span /></button>;
}

function StatusDot({ online }: { online: boolean }) {
  return <span className={`status-dot ${online ? "online" : "offline"}`} aria-hidden="true" />;
}

function HostCard({ host, active, local, busy, onSwitch }: {
  host: AppSnapshot["localHost"];
  active: boolean;
  local: boolean;
  busy: boolean;
  onSwitch: () => void;
}) {
  const Icon = host.platform === "macos" ? Laptop : Monitor;
  const platformName = host.platform === "macos" ? "macOS" : "Windows";

  return <article className={`host-card ${active ? "is-owner" : ""}`}>
    <div className="host-icon"><Icon size={23} /></div>
    <div className="host-copy">
      <div className="host-title">
        <h2>{host.name}</h2>
        {local && <span className="local-badge">本机</span>}
      </div>
      <p><StatusDot online={host.online} /> {host.online ? "在线" : "离线"} · {platformName} · 通道 {host.channel}</p>
    </div>
    {active ? <span className="owner-label"><Check size={15} /> 正在使用</span> : <button className="switch-button" disabled={!host.online || busy} onClick={onSwitch}>{busy ? <LoaderCircle className="spin" size={16} /> : null}切到这里</button>}
  </article>;
}

function SettingRow({ title, detail, children }: { title: string; detail: string; children: React.ReactNode }) {
  return <div className="setting-row"><div><strong>{title}</strong><span>{detail}</span></div>{children}</div>;
}

function SettingsDialog({ config, onClose, onSave }: { config: AppConfig; onClose: () => void; onSave: (config: AppConfig) => Promise<void> }) {
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

  return <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="settings-dialog" role="dialog" aria-modal="true" aria-label="KeyClaim 设置" onMouseDown={(event) => event.stopPropagation()}>
      <header><div><h2>设置</h2><p>两台电脑的配对码必须一致。</p></div><button className="icon-button" onClick={onClose} aria-label="关闭设置"><X size={20} /></button></header>
      <div className="settings-form">
        <label><span>本机名称</span><input value={draft.hostName} onChange={(event) => setDraft({ ...draft, hostName: event.target.value })} /></label>
        <label><span>本机通道</span><select value={draft.channel} onChange={(event) => { const channel = Number(event.target.value); setDraft({ ...draft, channel, peerChannel: channel === 3 ? 2 : 3 }); }}><option value={2}>通道 2</option><option value={3}>通道 3</option></select></label>
        <label><span>双端配对码</span><input value={draft.pairingCode} onChange={(event) => setDraft({ ...draft, pairingCode: event.target.value })} /></label>
        <label><span>指针灵敏度</span><select value={draft.movementThreshold} onChange={(event) => setDraft({ ...draft, movementThreshold: Number(event.target.value) })}><option value={2}>2px · 灵敏</option><option value={3}>3px · 推荐</option><option value={6}>6px · 稳健</option></select></label>
      </div>
      <footer><button className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={saving} onClick={submit}>{saving && <LoaderCircle className="spin" size={16} />}保存</button></footer>
    </section>
  </div>;
}

function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(mockLoading);
  const [loading, setLoading] = useState(true);
  const [busyChannel, setBusyChannel] = useState<number>();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [message, setMessage] = useState<string>();
  const [appVersion, setAppVersion] = useState("0.1.3");

  useEffect(() => {
    let mounted = true;
    loadSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => { if (mounted) setMessage("读取本机状态失败"); }).finally(() => { if (mounted) setLoading(false); });
    getAppVersion().then((version) => { if (mounted) setAppVersion(version); }).catch(() => undefined);
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

  const owner = useMemo(() => {
    const channel = snapshot.keyboard.currentChannel;
    if (channel === snapshot.localHost.channel) return snapshot.localHost;
    if (channel === snapshot.peerHost.channel) return snapshot.peerHost;
    return undefined;
  }, [snapshot]);

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

  const handleSwitch = async (channel: number) => {
    setBusyChannel(channel);
    setMessage(undefined);
    try {
      setSnapshot(await switchKeyboard(channel));
    } catch {
      setMessage("键盘切换失败，请确认接收器和目标电脑在线");
    } finally {
      window.setTimeout(() => setBusyChannel(undefined), 450);
    }
  };

  const handleRefresh = async () => {
    setLoading(true);
    try {
      setSnapshot(await refreshHardware());
      setMessage(undefined);
    } catch {
      setMessage("没有检测到键盘接收器");
    } finally {
      setLoading(false);
    }
  };

  const servicesReady = snapshot.diagnostics.inputMonitor && snapshot.diagnostics.networkListener;
  const latestActivity = snapshot.activities[0];

  return <div className="app-shell">
    <header className="app-header">
      <div className="brand"><img src="/keyclaim-icon.png" alt="" /><div><strong className="brand-name">KeyClaim <span className="version">v{appVersion}</span></strong><span>键盘自动归属</span></div></div>
      <div className="header-actions"><span className={`service-badge ${servicesReady ? "ready" : ""}`}><StatusDot online={servicesReady} />{servicesReady ? "服务正常" : "正在连接"}</span><button className="icon-button" onClick={() => setSettingsOpen(true)} aria-label="打开设置"><Settings size={20} /></button></div>
    </header>

    <main>
      <section className="ownership-card">
        <div className="keyboard-icon"><Keyboard size={28} /></div>
        <div className="ownership-copy"><span>当前键盘归属</span><h1>{owner?.name ?? (snapshot.keyboard.connected ? "正在识别" : "尚未连接")}</h1><p>{snapshot.keyboard.connected ? `MX Keys · ${snapshot.keyboard.receiverName ?? "Logitech 接收器"}` : "唤醒键盘后会自动识别当前通道"}</p></div>
        <div className="ownership-state"><strong>{snapshot.config.autoRecall ? "自动跟随已开启" : "自动跟随已暂停"}</strong><span>移动正在使用的鼠标或触控板即可切换</span></div>
      </section>

      <section className="hosts" aria-label="电脑列表">
        <HostCard host={snapshot.localHost} local active={owner?.channel === snapshot.localHost.channel} busy={busyChannel === snapshot.localHost.channel} onSwitch={() => handleSwitch(snapshot.localHost.channel)} />
        <HostCard host={snapshot.peerHost} local={false} active={owner?.channel === snapshot.peerHost.channel} busy={busyChannel === snapshot.peerHost.channel} onSwitch={() => handleSwitch(snapshot.peerHost.channel)} />
      </section>

      <section className="quick-settings" aria-label="常用设置">
        <SettingRow title="自动跟随" detail="检测到本机鼠标或触控板后召回键盘"><Toggle checked={snapshot.config.autoRecall} disabled={loading} onChange={(autoRecall) => updateQuickSetting({ autoRecall })} label="自动跟随" /></SettingRow>
        <SettingRow title="开机自动运行" detail="登录系统后自动在后台启动 KeyClaim"><Toggle checked={snapshot.config.launchAtLogin} disabled={loading} onChange={(launchAtLogin) => updateQuickSetting({ launchAtLogin })} label="开机自动运行" /></SettingRow>
      </section>

      {message && <div className="error-message" role="status"><CircleAlert size={18} />{message}</div>}

      <footer className="status-footer">
        <div className="health"><span><MousePointer2 size={17} />输入监听 {snapshot.diagnostics.inputMonitor ? "正常" : "未就绪"}</span><span><Wifi size={17} />另一台电脑 {snapshot.peerHost.online ? "在线" : "离线"}</span><span><ShieldCheck size={17} />仅在本地运行</span></div>
        <button className="refresh-button" onClick={handleRefresh} disabled={loading}>{loading ? <LoaderCircle className="spin" size={17} /> : <RefreshCw size={17} />}重新检测</button>
      </footer>

      {latestActivity && <p className="latest-activity"><span>最近</span>{latestActivity.title} · {latestActivity.detail}<time>{latestActivity.timestamp}</time></p>}
    </main>

    {settingsOpen && <SettingsDialog config={snapshot.config} onClose={() => setSettingsOpen(false)} onSave={async (config) => { await save(config); setSettingsOpen(false); }} />}
  </div>;
}

export default App;
