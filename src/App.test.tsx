import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { getChannelHosts, type AppSnapshot } from "./api";

afterEach(() => {
  cleanup();
  window.history.replaceState({}, "", "/");
  vi.restoreAllMocks();
});

describe("KeyClaim", () => {
  it("显示实际配置的通道和对应设备", async () => {
    render(<App />);
    const channelStrip = screen.getByRole("region", { name: "键盘通道" });
    expect(within(channelStrip).getAllByRole("article")).toHaveLength(2);
    expect(await within(channelStrip).findByText("MacBook Pro")).toBeVisible();
    expect(within(channelStrip).getByText("Windows 工作站")).toBeVisible();
    expect(within(channelStrip).queryByText("通道 1")).not.toBeInTheDocument();
  });

  it("切换自动跟随", async () => {
    render(<App />);
    const toggle = screen.getByRole("switch", { name: "自动跟随" });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });

  it("从顶部直接打开设置", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(screen.getByRole("dialog", { name: "KeyClaim · 键来设置" })).toBeVisible();
    expect(screen.getByRole("button", { name: "退出键来" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "关闭设置" }));
    expect(screen.queryByRole("dialog", { name: "KeyClaim · 键来设置" })).not.toBeInTheDocument();
  });

  it("同时展示英文品牌与中文名称", () => {
    render(<App />);
    expect(screen.getByText("KeyClaim")).toBeVisible();
    expect(screen.getByText("键来")).toBeVisible();
  });

  it("Windows 使用完整主界面直接展示设备设置", async () => {
    vi.spyOn(window.navigator, "userAgent", "get").mockReturnValue("Windows");
    window.history.replaceState({}, "", "/?platform=windows");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "设备设置" })).toBeVisible();
    expect(await screen.findByText("后台运行中")).toBeVisible();
    expect(screen.getByLabelText("常用设置")).toBeVisible();
    const nameField = screen.getByDisplayValue("Windows PC");
    const saveButton = screen.getByRole("button", { name: "保存更改" });
    expect(nameField).toBeVisible();
    expect(saveButton).toBeDisabled();
    expect(screen.queryByRole("button", { name: "打开设置" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "退出键来" })).not.toBeInTheDocument();

    fireEvent.change(nameField, { target: { value: "Feige QA" } });
    await waitFor(() => expect(saveButton).toBeEnabled());
    fireEvent.click(saveButton);
    await waitFor(() => expect(saveButton).toBeDisabled());
  });

  it("三台主机只渲染三个实际通道并按编号排序", () => {
    const snapshot = {
      localHost: { name: "MacBook", platform: "macos", channel: 3, online: true, ready: true },
      peerHost: { name: "Windows", platform: "windows", channel: 2, online: true, ready: true },
      hosts: [
        { name: "MacBook", platform: "macos", channel: 3, online: true, ready: true },
        { name: "Windows", platform: "windows", channel: 2, online: true, ready: true },
        { name: "iPad", platform: "macos", channel: 1, online: true, ready: true },
      ],
    } as AppSnapshot;

    expect(getChannelHosts(snapshot).map((host) => host.channel)).toEqual([1, 2, 3]);
  });
});
