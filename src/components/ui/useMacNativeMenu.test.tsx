import { useRef } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMacNativeMenu } from "./useMacNativeMenu";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), isTauri: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => bridge);

function Menu({ onSelect, onClose }: { onSelect: () => void; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const native = useMacNativeMenu(ref, true, onClose);
  return <div ref={ref} data-native-menu-source={native}>
    <button onClick={onSelect}>打开文件</button>
    <div className="ide-tab-context-menu-sep" />
    <button disabled>不可用</button>
  </div>;
}

describe("native menu source", () => {
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

  it("uses NSMenu rows and routes a native selection to the existing button", async () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(<Menu onSelect={onSelect} onClose={onClose} />);
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("show_macos_menu", expect.objectContaining({
      items: [
        expect.objectContaining({ label: "打开文件", id: "item-0" }),
        expect.objectContaining({ separator: true }),
        expect.objectContaining({ label: "不可用", disabled: true }),
      ],
    })));
    const options = bridge.invoke.mock.calls[0][1];
    window.dispatchEvent(new CustomEvent("harmony:native-menu-select", { detail: { menuId: options.menuId, id: "item-0" } }));
    window.dispatchEvent(new CustomEvent("harmony:native-menu-close", { detail: { menuId: options.menuId } }));
    expect(onSelect).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps the DOM menu on Windows", () => {
    Object.defineProperty(navigator, "userAgent", { configurable: true, value: "Windows" });
    render(<Menu onSelect={vi.fn()} onClose={vi.fn()} />);
    expect(screen.getByRole("button", { name: "打开文件" })).toBeInTheDocument();
    expect(bridge.invoke).not.toHaveBeenCalled();
  });
});
