import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("KeyClaim", () => {
  it("显示当前归属并切换自动跟随", async () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "MacBook Pro" })).toBeVisible();
    const toggle = screen.getByRole("switch", { name: "自动跟随" });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });

  it("从顶部直接打开设置", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "打开设置" }));
    expect(screen.getByRole("dialog", { name: "KeyClaim 设置" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "关闭设置" }));
    expect(screen.queryByRole("dialog", { name: "KeyClaim 设置" })).not.toBeInTheDocument();
  });

  it("在产品名称旁显示版本号", async () => {
    render(<App />);
    expect(await screen.findByText("v0.1.5")).toBeVisible();
  });
});
