import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as api from "./api";
import { getChannelHosts, type AppSnapshot } from "./api";

let snapshot: AppSnapshot;

beforeEach(() => {
  snapshot = {
    config: { hostId: "test-host", hostName: "MacBook Pro", platform: "macos", channel: 3, peerChannel: 2, pairingCode: "test-only-pairing", autoRecall: true, launchAtLogin: false, movementThreshold: 3, debounceMs: 60, networkPort: 45823, ignoreSyntheticInput: true },
    localHost: { name: "MacBook Pro", platform: "macos", channel: 3, online: true, ready: true },
    peerHost: { name: "Windows 工作站", platform: "windows", channel: 2, online: true, ready: true },
    keyboard: { connected: true, currentChannel: 3, supportsChangeHost: true },
    diagnostics: { inputMonitor: true, networkListener: true, hidAccess: true, receiverProbe: true, syntheticFilterMode: "test" },
    activities: [],
    pendingClaim: false,
  };
  vi.spyOn(api, "loadSnapshot").mockImplementation(async () => structuredClone(snapshot));
  vi.spyOn(api, "getSnapshot").mockImplementation(async () => structuredClone(snapshot));
  vi.spyOn(api, "getAppVersion").mockResolvedValue(api.CURRENT_VERSION);
  vi.spyOn(api, "saveConfig").mockImplementation(async (config) => {
    snapshot.config = structuredClone(config);
    snapshot.localHost.name = config.hostName;
    snapshot.localHost.channel = config.channel;
    snapshot.peerHost.channel = config.peerChannel;
    return structuredClone(snapshot);
  });
});

afterEach(() => {
  cleanup();
  window.history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});

async function renderReady(windows = false) {
  if (windows) {
    snapshot.config = { ...snapshot.config, platform: "windows", hostName: "Windows PC", channel: 2, peerChannel: 3 };
    snapshot.localHost = { ...snapshot.localHost, platform: "windows", name: "Windows PC", channel: 2 };
    snapshot.peerHost = { ...snapshot.peerHost, platform: "macos", name: "MacBook Pro", channel: 3 };
    snapshot.keyboard.currentChannel = 2;
  }
  render(<App />);
  await waitFor(() => expect(screen.getByRole("button", { name: "打开设置" })).toBeEnabled());
}

describe("KeyClaim", () => {
  it("显示实际配置的通道和对应设备", async () => {
    await renderReady();
    const channelStrip = screen.getByRole("region", { name: "键盘通道" });
    expect(within(channelStrip).getAllByRole("article")).toHaveLength(2);
    expect(within(channelStrip).getByText("MacBook Pro")).toBeVisible();
    expect(within(channelStrip).getByText("Windows 工作站")).toBeVisible();
    expect(within(channelStrip).queryByText("通道 1")).not.toBeInTheDocument();
  });

  it("切换自动跟随时保留其他配置", async () => {
    await renderReady(true);
    const before = structuredClone(snapshot.config);
    fireEvent.click(screen.getByRole("switch", { name: "自动跟随" }));
    await waitFor(() => expect(screen.getByRole("switch", { name: "自动跟随" })).toHaveAttribute("aria-checked", "false"));
    expect(api.saveConfig).toHaveBeenCalledWith({ ...before, autoRecall: false });
  });

  it("Mac 从顶部打开原有设置面板", async () => {
    await renderReady();
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(screen.getByRole("dialog", { name: "KeyClaim · 键来设置" })).toBeVisible();
    expect(screen.getByRole("button", { name: "退出键来" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "关闭设置" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("显示品牌与当前版本号", async () => {
    await renderReady();
    expect(screen.getByText("KeyClaim")).toBeVisible();
    expect(screen.getByText("键来")).toBeVisible();
    expect(screen.getByText(`v${api.CURRENT_VERSION}`)).toBeVisible();
  });

  it("Windows 主界面只显示通道与常用开关，设备设置放入右上角", async () => {
    await renderReady(true);
    expect(screen.getByText("Windows PC")).toBeVisible();
    expect(screen.getByLabelText("常用设置")).toBeVisible();
    expect(screen.getAllByRole("switch")).toHaveLength(2);
    for (const field of ["本机名称", "本机通道", "双端配对码", "指针灵敏度"]) {
      expect(screen.queryByLabelText(field)).not.toBeInTheDocument();
    }
    expect(screen.queryByRole("button", { name: "保存更改" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    for (const field of ["本机名称", "本机通道", "双端配对码", "指针灵敏度"]) {
      expect(screen.getByLabelText(field)).toBeVisible();
    }
    fireEvent.change(screen.getByLabelText("本机名称"), { target: { value: "Windows QA" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Windows QA")).toBeVisible();
    expect(snapshot.config.channel).toBe(2);
    expect(snapshot.config.peerChannel).toBe(3);
    expect(snapshot.config.autoRecall).toBe(true);
  });

  it("取消后丢弃未保存输入，下次打开仍显示原配置", async () => {
    await renderReady(true);
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    fireEvent.change(screen.getByLabelText("本机名称"), { target: { value: "未保存" } });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(api.saveConfig).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(screen.getByLabelText("本机名称")).toHaveValue("Windows PC");
  });

  it("Windows 支持 Ctrl+, 打开设置与 Escape 返回", async () => {
    await renderReady(true);
    fireEvent.keyDown(window, { key: ",", ctrlKey: true });
    const dialog = screen.getByRole("dialog");
    expect(dialog).toBeVisible();
    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("保存失败时保留编辑并在设置里提示，可以重试", async () => {
    await renderReady(true);
    vi.mocked(api.saveConfig).mockRejectedValueOnce(new Error("test failure"));
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    fireEvent.change(screen.getByLabelText("本机名称"), { target: { value: "Retry name" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(await screen.findByRole("status")).toHaveTextContent("设置没有保存成功");
    expect(screen.getByLabelText("本机名称")).toHaveValue("Retry name");
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("Retry name")).toBeVisible();
  });

  it("保存期间不允许重复提交或通过 Escape 关闭设置", async () => {
    await renderReady(true);
    let finishSave!: (value: AppSnapshot) => void;
    vi.mocked(api.saveConfig).mockImplementationOnce(() => new Promise((resolve) => { finishSave = resolve; }));
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeVisible();
    finishSave(structuredClone(snapshot));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("后台刷新不覆盖编辑，保存也不回滚实时的常用开关", async () => {
    await renderReady(true);
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    fireEvent.change(screen.getByLabelText("本机名称"), { target: { value: "Editing name" } });
    snapshot.config.autoRecall = false;
    await waitFor(() => expect(api.getSnapshot).toHaveBeenCalled(), { timeout: 1500 });
    expect(screen.getByLabelText("本机名称")).toHaveValue("Editing name");
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(snapshot.config.autoRecall).toBe(false);
  });

  it("三台主机只渲染三个实际通道并按编号排序", () => {
    snapshot.hosts = [
      { name: "MacBook", platform: "macos", channel: 3, online: true, ready: true },
      { name: "Windows", platform: "windows", channel: 2, online: true, ready: true },
      { name: "iPad", platform: "macos", channel: 1, online: true, ready: true },
    ];
    expect(getChannelHosts(snapshot).map((host) => host.channel)).toEqual([1, 2, 3]);
  });
});
