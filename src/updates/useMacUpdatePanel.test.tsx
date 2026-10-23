import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useMacUpdatePanel } from "./useMacUpdatePanel";
import type { UpdateSnapshot, UpdateStore } from "./updateStore";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("../components/ui/useMacNativeMenu", () => ({ isMacNativeUI: () => true }));
afterEach(() => { cleanup(); invoke.mockReset().mockResolvedValue(undefined); });
const initial: UpdateSnapshot = { phase: "checking", currentVersion: "1.1.2", available: null, channel: "stable", downloaded: 0, total: null, error: null, nextCheckAt: null };
it("updates the AppKit panel without closing it between progress events and routes native actions", async () => {
  const store = { check: vi.fn(), install: vi.fn(), cancel: vi.fn(), setChannel: vi.fn() } as unknown as UpdateStore;
  const onClose = vi.fn();
  const h = renderHook(({ state }) => useMacUpdatePanel(state, store, onClose), { initialProps: { state: initial } });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("show_macos_update", { snapshot: initial }));
  const downloading = { ...initial, phase: "downloading" as const, downloaded: 10, total: 100 };
  h.rerender({ state: downloading });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("show_macos_update", { snapshot: downloading }));
  expect(invoke.mock.calls.some(([command]) => command === "close_macos_update")).toBe(false);
  for (const action of ["check", "install", "cancel", "preview", "stable", "close"]) {
    act(() => window.dispatchEvent(new CustomEvent("harmony:native-update-action", { detail: { action } })));
  }
  expect(store.check).toHaveBeenCalledOnce();
  expect(store.install).toHaveBeenCalledOnce();
  expect(store.cancel).toHaveBeenCalledOnce();
  expect(store.setChannel).toHaveBeenNthCalledWith(1, "preview");
  expect(store.setChannel).toHaveBeenNthCalledWith(2, "stable");
  expect(onClose).toHaveBeenCalledOnce();
  h.unmount();
  expect(invoke).toHaveBeenCalledWith("close_macos_update");
});
it("falls back when the native panel cannot open", async () => {
  invoke.mockRejectedValueOnce(new Error("native unavailable"));
  const h = renderHook(() => useMacUpdatePanel(initial, {} as UpdateStore, vi.fn()));
  await waitFor(() => expect(h.result.current).toBe(false));
});
