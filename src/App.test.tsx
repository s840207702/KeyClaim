import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("KeyClaim", () => {
  it("renders the ownership overview and toggles automatic recall", async () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "键盘会跟随你的工作位置" })).toBeVisible();
    const toggle = screen.getByRole("switch", { name: "自动召回" });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });

  it("opens settings and shows diagnostics", () => {
    render(<App />);
    const nav = screen.getByRole("navigation", { name: "主导航" });
    fireEvent.click(within(nav).getByRole("button", { name: "设置" }));
    expect(screen.getByRole("dialog", { name: "KeyClaim 设置" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "关闭设置" }));
    fireEvent.click(within(nav).getByRole("button", { name: "诊断" }));
    expect(screen.getByRole("heading", { name: "运行诊断" })).toBeVisible();
  });
});
