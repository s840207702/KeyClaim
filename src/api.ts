import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { LazyStore } from "@tauri-apps/plugin-store";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

export type Platform = "macos" | "windows";

export interface AppConfig {
  hostId: string;
  hostName: string;
  platform: Platform;
  channel: number;
  peerChannel: number;
  pairingCode: string;
  autoRecall: boolean;
  launchAtLogin: boolean;
  movementThreshold: number;
  debounceMs: number;
  networkPort: number;
  ignoreSyntheticInput: boolean;
}

export interface HostView {
  name: string;
  platform: Platform;
  channel: number;
  online: boolean;
  ready: boolean;
  lastSeenMs?: number;
}

export interface AppSnapshot {
  config: AppConfig;
  localHost: HostView;
  peerHost: HostView;
  keyboard: {
    connected: boolean;
    receiverName?: string;
    currentChannel?: number;
    supportsChangeHost: boolean;
    lastError?: string;
  };
  diagnostics: {
    inputMonitor: boolean;
    networkListener: boolean;
    hidAccess: boolean;
    receiverProbe: boolean;
    syntheticFilterMode: string;
  };
  activities: Array<{
    id: string;
    timestamp: string;
    title: string;
    detail: string;
    kind: string;
  }>;
  lastLocalInputMs?: number;
  pendingClaim: boolean;
}

const browserSnapshot: AppSnapshot = {
  config: {
    hostId: "browser-preview",
    hostName: "MacBook Pro",
    platform: "macos",
    channel: 3,
    peerChannel: 2,
    pairingCode: "KEYCLAIM",
    autoRecall: true,
    launchAtLogin: false,
    movementThreshold: 3,
    debounceMs: 60,
    networkPort: 45823,
    ignoreSyntheticInput: true,
  },
  localHost: {
    name: "MacBook Pro",
    platform: "macos",
    channel: 3,
    online: true,
    ready: true,
    lastSeenMs: Date.now(),
  },
  peerHost: {
    name: "Windows 工作站",
    platform: "windows",
    channel: 2,
    online: true,
    ready: true,
    lastSeenMs: Date.now() - 820,
  },
  keyboard: {
    connected: true,
    receiverName: "Logitech USB Receiver",
    currentChannel: 3,
    supportsChangeHost: true,
  },
  diagnostics: {
    inputMonitor: true,
    networkListener: true,
    hidAccess: true,
    receiverProbe: true,
    syntheticFilterMode: "macOS CGEventTap · 过滤合成事件",
  },
  activities: [
    { id: "1", timestamp: "10:42:18", title: "键盘已归位", detail: "MX Keys → MacBook Pro · 通道 3", kind: "success" },
    { id: "2", timestamp: "10:42:17", title: "检测到本地输入", detail: "触控板移动 · 目标主机已就绪", kind: "claim" },
    { id: "3", timestamp: "10:36:04", title: "另一台电脑已就绪", detail: "Windows 工作站 · 局域网握手成功", kind: "network" },
  ],
  lastLocalInputMs: Date.now() - 1_200,
  pendingClaim: false,
};

const isTauri = () => "__TAURI_INTERNALS__" in window;
const store = isTauri() ? new LazyStore("keyclaim-settings.json") : null;

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return structuredClone(browserSnapshot) as T;
  return invoke<T>(command, args);
}

export async function loadSnapshot(): Promise<AppSnapshot> {
  const initial = await call<AppSnapshot>("get_snapshot");
  if (!store) return initial;
  const saved = await store.get<AppConfig>("config");
  const launchAtLogin = await isEnabled().catch(() => false);
  const config = { ...initial.config, ...saved, launchAtLogin };
  return call<AppSnapshot>("update_config", { config });
}

export const getSnapshot = () => call<AppSnapshot>("get_snapshot");
export const getAppVersion = () => isTauri() ? getVersion() : Promise.resolve("0.2.1");
export const refreshHardware = () => call<AppSnapshot>("refresh_hardware");
export const testRecall = () => call<AppSnapshot>("test_recall");
export const switchKeyboard = (channel: number) => call<AppSnapshot>("switch_keyboard", { channel });
export const requestHidAccess = () => call<AppSnapshot>("request_hid_access");

export async function saveConfig(config: AppConfig): Promise<AppSnapshot> {
  if (store) {
    const enabled = await isEnabled().catch(() => false);
    if (config.launchAtLogin !== enabled) {
      if (config.launchAtLogin) await enable(); else await disable();
    }
    await store.set("config", config);
  }
  if (!isTauri()) {
    Object.assign(browserSnapshot.config, config);
    browserSnapshot.localHost.channel = config.channel;
    browserSnapshot.peerHost.channel = config.peerChannel;
  }
  return call<AppSnapshot>("update_config", { config });
}
