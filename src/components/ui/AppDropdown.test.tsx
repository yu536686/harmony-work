import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import AppDropdown from "./AppDropdown";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => bridge);

const items = [
  { id: "one", label: "第一项", selected: true },
  { id: "two", label: "第二项" },
];

describe("AppDropdown native macOS bridge", () => {
  const originalUserAgent = navigator.userAgent;

  beforeEach(() => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Macintosh" });
    bridge.isTauri.mockReturnValue(true);
    bridge.invoke.mockResolvedValue(undefined);
  });

  afterEach(() => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: originalUserAgent });
    vi.clearAllMocks();
  });

  it("opens a native popover and routes selection through the existing handler", async () => {
    const onSelect = vi.fn();
    const onOpenChange = vi.fn();
    render(<AppDropdown menuId="model" triggerLabel="模型" items={items} open onSelect={onSelect} onOpenChange={onOpenChange} />);

    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_dropdown", expect.objectContaining({
      menuId: "model",
      items: expect.arrayContaining([expect.objectContaining({ id: "one", selected: true })]),
    })));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();

    window.dispatchEvent(new CustomEvent("harmony:native-dropdown-select", { detail: { menuId: "model", id: "two" } }));
    expect(onSelect).toHaveBeenCalledWith("two");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("falls back to the WebView list when the native bridge fails", async () => {
    bridge.invoke.mockRejectedValue(new Error("Unavailable"));
    render(<AppDropdown menuId="model" triggerLabel="模型" items={items} open onSelect={vi.fn()} onOpenChange={vi.fn()} />);

    expect(await screen.findByRole("listbox")).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "第二项" })).toBeVisible();
  });

  it("updates native checkmarks without closing a multi-select menu", async () => {
    const onOpenChange = vi.fn();
    const { rerender } = render(<AppDropdown menuId="skills" triggerLabel="技能" items={items} open closeOnSelect={false} onSelect={vi.fn()} onOpenChange={onOpenChange} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_dropdown", expect.anything()));

    rerender(<AppDropdown menuId="skills" triggerLabel="技能" items={[items[0], { ...items[1], selected: true }]} open closeOnSelect={false} onSelect={vi.fn()} onOpenChange={onOpenChange} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("update_macos_dropdown", expect.objectContaining({
      menuId: "skills",
      items: expect.arrayContaining([expect.objectContaining({ id: "two", selected: true })]),
    })));
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("keeps the existing WebView path on Windows", () => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Windows" });
    render(<AppDropdown menuId="model" triggerLabel="模型" items={items} open onSelect={vi.fn()} onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("option", { name: "第二项" }));
    expect(bridge.invoke).not.toHaveBeenCalled();
  });
});
