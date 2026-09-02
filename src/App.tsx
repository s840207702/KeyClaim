import { useEffect, useState } from "react";
import {
  Activity, ArrowRight, Check, ChevronDown, CircleHelp, Command, Gauge,
  Keyboard, Laptop, LayoutDashboard, LoaderCircle, Monitor, Mouse, Network,
  PanelLeftClose, Radio, RefreshCw, RotateCcw, Settings, ShieldCheck,
  SlidersHorizontal, Sparkles, Touchpad, X, Zap,
} from "lucide-react";
import "./App.css";
import {
  AppConfig, AppSnapshot, getSnapshot, loadSnapshot, refreshHardware, saveConfig,
  switchKeyboard, testRecall,
} from "./api";

type Page = "overview" | "devices" | "rules" | "diagnostics" | "settings";

const navigation: Array<{ id: Page; label: string; icon: typeof Activity }> = [
  { id: "overview", label: "总览", icon: LayoutDashboard },
  { id: "devices", label: "设备", icon: Keyboard },
  { id: "rules", label: "规则", icon: SlidersHorizontal },
  { id: "diagnostics", label: "诊断", icon: Activity },
  { id: "settings", label: "设置", icon: Settings },
];

const mockLoading: AppSnapshot = {
  config: { hostId: "loading", hostName: "本机", platform: "macos", channel: 3, peerChannel: 2, pairingCode: "KEYCLAIM", autoRecall: true, movementThreshold: 3, debounceMs: 60, networkPort: 45823, ignoreSyntheticInput: true },
  localHost: { name: "MacBook Pro", platform: "macos", channel: 3, online: true, ready: false },
  peerHost: { name: "Windows 工作站", platform: "windows", channel: 2, online: false, ready: false },
  keyboard: { connected: false, supportsChangeHost: false },
  diagnostics: { inputMonitor: false, networkListener: false, receiverProbe: false, syntheticFilterMode: "正在初始化" },
  activities: [], pendingClaim: false,
};

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (value: boolean) => void; label: string }) {
  return <button type="button" className={`toggle ${checked ? "is-on" : ""}`} role="switch" aria-checked={checked} aria-label={label} onClick={() => onChange(!checked)}><span /></button>;
}

function StatusDot({ online, pulse = false }: { online: boolean; pulse?: boolean }) {
  return <span className={`status-dot ${online ? "online" : "offline"} ${pulse ? "pulse" : ""}`} aria-hidden="true" />;
}

function Sidebar({ page, setPage }: { page: Page; setPage: (page: Page) => void }) {
  return <aside className="sidebar">
    <div className="traffic-lights" aria-hidden="true"><i /><i /><i /></div>
    <div className="brand"><div className="brand-mark"><Command size={17} strokeWidth={2.2} /></div><div><strong>KeyClaim</strong><span>键盘归属助手</span></div></div>
    <nav aria-label="主导航">{navigation.map((item) => { const Icon = item.icon; return <button key={item.id} className={page === item.id ? "active" : ""} onClick={() => setPage(item.id)}><Icon size={18} /><span>{item.label}</span></button>; })}</nav>
    <div className="sidebar-spacer" />
    <div className="quiet-card"><ShieldCheck size={18} /><div><strong>本地优先</strong><span>输入和设备状态不会上传</span></div></div>
    <button className="sidebar-help"><CircleHelp size={17} /> 工作原理</button>
    <div className="sidebar-foot"><span className="avatar">非</span><div><strong>非哥的设备组</strong><span>2 台电脑</span></div><ChevronDown size={15} /></div>
  </aside>;
}

function Header({ snapshot, onToggle }: { snapshot: AppSnapshot; onToggle: (value: boolean) => void }) {
  return <header className="topbar">
    <div><h1>键盘会跟随你的工作位置</h1><p>移动本机鼠标或触控板，MX Keys 就会自动归位。</p></div>
    <div className="topbar-actions"><div className="auto-control"><div><span>自动召回</span><small>{snapshot.config.autoRecall ? "正在运行" : "已暂停"}</small></div><Toggle checked={snapshot.config.autoRecall} onChange={onToggle} label="自动召回" /></div><button className="icon-button" aria-label="折叠侧栏"><PanelLeftClose size={18} /></button></div>
  </header>;
}

function DeviceNode({ type, title, channel, online, detail, active }: { type: "windows" | "mac" | "keyboard"; title: string; channel?: number; online: boolean; detail: React.ReactNode; active?: boolean }) {
  const Icon = type === "windows" ? Monitor : type === "mac" ? Laptop : Keyboard;
  return <article className={`device-node ${type} ${active ? "active" : ""}`}>
    <div className="node-head"><div className="device-icon"><Icon size={24} /></div><StatusDot online={online} pulse={active} /></div>
    <div className="node-copy"><div className="node-title"><strong>{title}</strong>{channel && <span>通道 {channel}</span>}</div><div className="node-detail">{detail}</div></div>
    {active && type !== "keyboard" && <div className="owner-badge"><Check size={13} /> 当前归属</div>}
  </article>;
}

function Connection({ active, label }: { active: boolean; label: string }) {
  return <div className={`connection ${active ? "active" : ""}`}><span className="line"><i /></span><small>{label}</small></div>;
}

function Topology({ snapshot, busyChannel, onSwitch, onRefresh }: { snapshot: AppSnapshot; busyChannel?: number; onSwitch: (channel: number) => void; onRefresh: () => void }) {
  const keyboardChannel = snapshot.keyboard.currentChannel;
  const localIsMac = snapshot.config.platform === "macos";
  const windows = localIsMac ? snapshot.peerHost : snapshot.localHost;
  const mac = localIsMac ? snapshot.localHost : snapshot.peerHost;
  return <section className="topology-card panel">
    <div className="section-heading"><div><span className="eyebrow"><Radio size={14} /> 实时拓扑</span><h2>设备连接</h2></div><button className="text-button" onClick={onRefresh}><RefreshCw size={14} /> 重新扫描</button></div>
    <div className="topology">
      <DeviceNode type="windows" title="Windows 工作站" channel={windows.channel} online={windows.online} active={keyboardChannel === windows.channel} detail={<><Mouse size={14} /> MX Master 3 · 接收器</>} />
      <Connection active={keyboardChannel === windows.channel} label={windows.online ? "局域网在线" : "等待上线"} />
      <DeviceNode type="keyboard" title="MX Keys" online={snapshot.keyboard.connected} active={snapshot.keyboard.connected} detail={<>{snapshot.keyboard.connected ? snapshot.keyboard.receiverName ?? "Unifying 接收器" : "等待接收器"}</>} />
      <Connection active={keyboardChannel === mac.channel} label={mac.online ? "局域网在线" : "等待上线"} />
      <DeviceNode type="mac" title="MacBook Pro" channel={mac.channel} online={mac.online} active={keyboardChannel === mac.channel} detail={<span className="input-devices"><span><Mouse size={14} /> Master 3S</span><span><Touchpad size={14} /> 触控板</span></span>} />
    </div>
    <div className="topology-actions"><span><Sparkles size={15} /> 当前由 <strong>{keyboardChannel === 2 ? "Windows 工作站" : keyboardChannel === 3 ? "MacBook Pro" : "未知设备"}</strong> 使用</span><div><button className="secondary-button" disabled={busyChannel !== undefined} onClick={() => onSwitch(2)}>{busyChannel === 2 && <LoaderCircle className="spin" size={15} />} 切到 Windows</button><button className="primary-button" disabled={busyChannel !== undefined} onClick={() => onSwitch(3)}>{busyChannel === 3 && <LoaderCircle className="spin" size={15} />} 切到 Mac</button></div></div>
  </section>;
}

function StateRail({ snapshot }: { snapshot: AppSnapshot }) {
  const items = [
    { icon: Mouse, label: "最近输入", value: snapshot.lastLocalInputMs ? "刚刚 · 本机" : "等待输入", state: Boolean(snapshot.lastLocalInputMs) },
    { icon: Keyboard, label: "当前归属", value: snapshot.keyboard.currentChannel ? `通道 ${snapshot.keyboard.currentChannel}` : "未识别", state: snapshot.keyboard.connected },
    { icon: Network, label: "局域网", value: snapshot.peerHost.online ? "双端在线" : "仅本机在线", state: snapshot.peerHost.online },
    { icon: RotateCcw, label: "休眠恢复", value: snapshot.peerHost.ready ? "目标已就绪" : "等待目标主机", state: snapshot.peerHost.ready },
  ];
  return <section className="state-rail" aria-label="运行状态">{items.map(({ icon: Icon, label, value, state }) => <div key={label}><span className={`rail-icon ${state ? "good" : "muted"}`}><Icon size={17} /></span><div><small>{label}</small><strong>{value}</strong></div></div>)}</section>;
}

function RulesPanel({ snapshot }: { snapshot: AppSnapshot }) {
  const rules = [
    { icon: Mouse, title: "物理鼠标与触控板触发", desc: `位移超过 ${snapshot.config.movementThreshold}px 即判定本地工作`, enabled: true },
    { icon: ShieldCheck, title: "忽略远程控制与模拟输入", desc: "实验性过滤；系统级物理事件识别仍在完善", enabled: snapshot.config.ignoreSyntheticInput, warning: true },
    { icon: Network, title: "目标主机就绪后再切换", desc: "离线或休眠时不执行错误切换", enabled: true },
  ];
  return <section className="panel half-panel"><div className="section-heading compact"><div><span className="eyebrow"><Zap size={14} /> 生效中</span><h2>召回规则</h2></div><button className="text-button">管理规则 <ArrowRight size={14} /></button></div><div className="rule-list">{rules.map(({ icon: Icon, title, desc, enabled, warning }) => <div className="rule-item" key={title}><span className={`rule-icon ${warning ? "warning" : ""}`}><Icon size={17} /></span><div><strong>{title}</strong><span>{desc}</span></div><span className={`mini-state ${enabled ? "enabled" : ""}`}>{enabled ? <Check size={13} /> : <X size={13} />}</span></div>)}</div></section>;
}

function ActivityPanel({ snapshot }: { snapshot: AppSnapshot }) {
  return <section className="panel half-panel"><div className="section-heading compact"><div><span className="eyebrow"><Activity size={14} /> 实时记录</span><h2>最近活动</h2></div><button className="text-button">查看全部 <ArrowRight size={14} /></button></div><div className="activity-list">{snapshot.activities.length === 0 ? <div className="empty-state"><Activity size={22} /><strong>还没有切换记录</strong><span>移动鼠标后，召回过程会出现在这里。</span></div> : snapshot.activities.slice(0, 4).map((item) => <div className="activity-item" key={item.id}><span className={`activity-mark ${item.kind}`}><i /></span><div><strong>{item.title}</strong><span>{item.detail}</span></div><time>{item.timestamp}</time></div>)}</div></section>;
}

function Overview({ snapshot, busyChannel, onSwitch, onTest, onRefresh }: { snapshot: AppSnapshot; busyChannel?: number; onSwitch: (channel: number) => void; onTest: () => void; onRefresh: () => void }) {
  return <><Topology snapshot={snapshot} busyChannel={busyChannel} onSwitch={onSwitch} onRefresh={onRefresh} /><StateRail snapshot={snapshot} /><div className="two-column"><RulesPanel snapshot={snapshot} /><ActivityPanel snapshot={snapshot} /></div><button className="diagnostic-strip" onClick={onTest} disabled={!snapshot.peerHost.online || snapshot.pendingClaim}><span className="diagnostic-icon"><Gauge size={18} /></span><span><strong>{snapshot.peerHost.online ? "双端服务运行正常" : "等待另一台电脑上线"}</strong><small>{snapshot.peerHost.online ? "可以执行一次无破坏的召回测试" : "离线时不会向键盘发送切换指令"}</small></span><span className="test-action">{snapshot.pendingClaim ? <><LoaderCircle className="spin" size={15} /> 正在召回</> : <>测试召回 <ArrowRight size={14} /></>}</span></button></>;
}

function InfoCard({ title, rows }: { title: string; rows: string[][] }) {
  return <article className="info-card"><h3>{title}</h3>{rows.map(([label, value]) => <div key={label}><span>{label}</span><strong>{value}</strong></div>)}</article>;
}

function DetailPage({ page, snapshot, onOpenSettings }: { page: Page; snapshot: AppSnapshot; onOpenSettings: () => void }) {
  const content = {
    devices: { icon: Keyboard, eyebrow: "硬件", title: "设备与通道", desc: "确认两端接收器、MX Keys 通道和当前归属。" },
    rules: { icon: SlidersHorizontal, eyebrow: "自动化", title: "召回规则", desc: "只用状态判断和短防抖，不加入让人有体感的锁定时间。" },
    diagnostics: { icon: Activity, eyebrow: "系统", title: "运行诊断", desc: "查看输入监听、局域网和 HID 接收器的真实状态。" },
    settings: { icon: Settings, eyebrow: "偏好", title: "KeyClaim 设置", desc: "设置通道、配对码和输入灵敏度。" },
    overview: { icon: LayoutDashboard, eyebrow: "总览", title: "总览", desc: "" },
  }[page];
  const Icon = content.icon;
  return <section className="detail-page"><div className="detail-title"><span><Icon size={20} /></span><div><small>{content.eyebrow}</small><h2>{content.title}</h2><p>{content.desc}</p></div></div>
    {page === "devices" && <div className="detail-grid"><InfoCard title="MX Keys" rows={[["接收器", snapshot.keyboard.receiverName ?? "未识别"], ["当前通道", snapshot.keyboard.currentChannel ? `通道 ${snapshot.keyboard.currentChannel}` : "未知"], ["ChangeHost", snapshot.keyboard.supportsChangeHost ? "支持" : "未确认"]]} /><InfoCard title="电脑" rows={[["本机", `${snapshot.localHost.name} · 通道 ${snapshot.localHost.channel}`], ["另一台", `${snapshot.peerHost.name} · 通道 ${snapshot.peerHost.channel}`], ["双端状态", snapshot.peerHost.online ? "在线" : "离线"]]} /></div>}
    {page === "rules" && <div className="wide-card"><RulesPanel snapshot={snapshot} /></div>}
    {page === "diagnostics" && <div className="detail-grid"><InfoCard title="基础服务" rows={[["输入监听", snapshot.diagnostics.inputMonitor ? "正常" : "未启动"], ["局域网监听", snapshot.diagnostics.networkListener ? "正常" : "端口不可用"], ["接收器扫描", snapshot.diagnostics.receiverProbe ? "已完成" : "扫描中"]]} /><InfoCard title="输入可信度" rows={[["当前模式", snapshot.diagnostics.syntheticFilterMode], ["远程输入过滤", "实验性"], ["推荐", "保留键盘按键作为手动兜底"]]} /></div>}
    {page === "settings" && <div className="settings-cta"><Settings size={26} /><strong>调整这台电脑的角色</strong><span>通道与配对配置会保存在本机。</span><button className="primary-button" onClick={onOpenSettings}>打开设置</button></div>}
  </section>;
}

function SettingsSheet({ config, onClose, onSave }: { config: AppConfig; onClose: () => void; onSave: (config: AppConfig) => void }) {
  const [draft, setDraft] = useState(config);
  return <div className="modal-backdrop" role="presentation" onMouseDown={onClose}><section className="settings-sheet" role="dialog" aria-modal="true" aria-label="KeyClaim 设置" onMouseDown={(event) => event.stopPropagation()}>
    <header><div><span className="eyebrow">本机偏好</span><h2>设置</h2></div><button className="icon-button" onClick={onClose} aria-label="关闭设置"><X size={18} /></button></header>
    <div className="form-section"><h3>设备角色</h3><label><span>本机名称<small>会显示在另一台电脑上</small></span><input value={draft.hostName} onChange={(e) => setDraft({ ...draft, hostName: e.target.value })} /></label><label><span>本机通道<small>Mac 当前使用 3，Windows 使用 2</small></span><select value={draft.channel} onChange={(e) => setDraft({ ...draft, channel: Number(e.target.value), peerChannel: Number(e.target.value) === 3 ? 2 : 3 })}><option value={2}>通道 2</option><option value={3}>通道 3</option></select></label></div>
    <div className="form-section"><h3>双端握手</h3><label><span>配对码<small>两台电脑必须完全一致</small></span><input value={draft.pairingCode} onChange={(e) => setDraft({ ...draft, pairingCode: e.target.value })} /></label></div>
    <div className="form-section"><h3>输入判定</h3><label><span>位移阈值<small>越低越灵敏，建议 3px</small></span><select value={draft.movementThreshold} onChange={(e) => setDraft({ ...draft, movementThreshold: Number(e.target.value) })}><option value={2}>2px · 极灵敏</option><option value={3}>3px · 推荐</option><option value={6}>6px · 稳健</option></select></label><label className="switch-row"><span>自动召回<small>检测到本地输入后发起归属请求</small></span><Toggle checked={draft.autoRecall} onChange={(autoRecall) => setDraft({ ...draft, autoRecall })} label="自动召回" /></label></div>
    <footer><button className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" onClick={() => onSave(draft)}>保存设置</button></footer>
  </section></div>;
}

function App() {
  const [snapshot, setSnapshot] = useState<AppSnapshot>(mockLoading);
  const [page, setPage] = useState<Page>("overview");
  const [loading, setLoading] = useState(true);
  const [busyChannel, setBusyChannel] = useState<number>();
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => { let mounted = true; loadSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => undefined).finally(() => { if (mounted) setLoading(false); }); const timer = window.setInterval(() => getSnapshot().then((next) => { if (mounted) setSnapshot(next); }).catch(() => undefined), 900); return () => { mounted = false; window.clearInterval(timer); }; }, []);
  useEffect(() => { const listener = (event: KeyboardEvent) => { if (event.key === "," && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setSettingsOpen(true); } if (event.key === "Escape") setSettingsOpen(false); }; window.addEventListener("keydown", listener); return () => window.removeEventListener("keydown", listener); }, []);
  const statusLabel = snapshot.peerHost.online ? "双端在线" : "等待另一台电脑";
  const updateConfig = async (config: AppConfig) => setSnapshot(await saveConfig(config));
  const handleSwitch = async (channel: number) => { setBusyChannel(channel); try { setSnapshot(await switchKeyboard(channel)); } finally { window.setTimeout(() => setBusyChannel(undefined), 550); } };
  const handleTest = async () => setSnapshot(await testRecall());
  const handleRefresh = async () => setSnapshot(await refreshHardware());

  return <div className="app-shell"><Sidebar page={page} setPage={(next) => { setPage(next); if (next === "settings") setSettingsOpen(true); }} /><main className="main-content"><Header snapshot={snapshot} onToggle={(autoRecall) => updateConfig({ ...snapshot.config, autoRecall })} /><div className="content-scroll"><div className="live-pill"><StatusDot online={snapshot.peerHost.online} pulse /> {statusLabel}{loading && <LoaderCircle className="spin" size={13} />}</div>{page === "overview" ? <Overview snapshot={snapshot} busyChannel={busyChannel} onSwitch={handleSwitch} onTest={handleTest} onRefresh={handleRefresh} /> : <DetailPage page={page} snapshot={snapshot} onOpenSettings={() => setSettingsOpen(true)} />}</div></main>{settingsOpen && <SettingsSheet config={snapshot.config} onClose={() => setSettingsOpen(false)} onSave={async (config) => { await updateConfig(config); setSettingsOpen(false); }} />}</div>;
}

export default App;
