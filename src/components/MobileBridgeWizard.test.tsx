import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import MobileBridgeWizard, { mobileBridgeStorageKey, mobileBridgeTokenKey } from "./MobileBridgeWizard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), isTauri: vi.fn(() => true) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke, isTauri: bridge.isTauri }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(mobileBridgeStorageKey("account-1"), "desktop-1");
  localStorage.setItem(mobileBridgeTokenKey("account-1"), "T".repeat(43));
  bridge.invoke.mockReset();
  bridge.listen.mockReset();
  bridge.listen.mockResolvedValue(() => {});
  bridge.isTauri.mockReturnValue(true);
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Windows");
  Object.defineProperty(window, "confirm", { configurable: true, value: vi.fn(() => true) });
});

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

const phone = { desktopDeviceId: "desktop-1", mobileDeviceId: "phone-1", mobileName: "iPhone" };
const pairing = { pairingId: "pair-1", desktopDeviceId: "desktop-new", deviceToken: "N".repeat(43), secret: "one+time&secret", expiresAt: "2026-10-23T12:05:00Z" };
function mockCommands() {
  bridge.invoke.mockImplementation(async (name: string) => {
    if (name === "mobile_bridge_devices") return { devices: [phone, { ...phone, desktopDeviceId: "other-desktop", mobileDeviceId: "other-phone" }] };
    if (name === "mobile_bridge_start_pairing") return pairing;
    if (name === "mobile_bridge_pairing_status") return { status: "pending", mobileDeviceId: null };
    return {};
  });
}
function nativeState() {
  const calls = bridge.invoke.mock.calls.filter(([name]) => name === "show_macos_mobile_bridge" || name === "update_macos_mobile_bridge");
  return calls[calls.length - 1]?.[1];
}
async function openNative(props: Partial<Parameters<typeof MobileBridgeWizard>[0]> = {}) {
  vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Macintosh");
  const callbacks = { onClose: vi.fn(), onBound: vi.fn(), onLinksChanged: vi.fn(), ...props };
  const view = render(<MobileBridgeWizard accountId="account-1" {...callbacks} />);
  await waitFor(() => expect(nativeState()?.state.phones).toEqual([phone]));
  return { ...view, ...callbacks, panelId: nativeState().panelId };
}
function nativeAction(panelId: string, action: string, mobileDeviceId?: string) {
  act(() => window.dispatchEvent(new CustomEvent("harmony:native-mobile-bridge-action", { detail: { panelId, action, mobileDeviceId } })));
}

it("uses an AppKit panel on Mac and sends only account-scoped display state", async () => {
  mockCommands();
  const view = await openNative();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(nativeState().state.linked).toBe(true);
  expect(JSON.stringify(nativeState())).not.toContain("T".repeat(43));
  nativeAction("another-panel", "generate");
  expect(bridge.invoke).not.toHaveBeenCalledWith("mobile_bridge_start_pairing");
  nativeAction(view.panelId, "generate");
  await waitFor(() => expect(nativeState().state.pairingUri).toBe("harmonywork://pair?id=pair-1&secret=one%2Btime%26secret"));
  expect(JSON.stringify(nativeState())).not.toContain(pairing.deviceToken);
  view.unmount();
  expect(bridge.invoke).toHaveBeenCalledWith("close_macos_mobile_bridge", { panelId: view.panelId });
});

it("routes native unlink only to a currently displayed phone without an HTML confirmation", async () => {
  mockCommands();
  const view = await openNative();
  nativeAction(view.panelId, "unlink", "other-phone");
  expect(bridge.invoke).not.toHaveBeenCalledWith("mobile_bridge_unlink", expect.anything());
  nativeAction(view.panelId, "unlink", phone.mobileDeviceId);
  await waitFor(() => expect(view.onLinksChanged).toHaveBeenCalledOnce());
  expect(bridge.invoke).toHaveBeenCalledWith("mobile_bridge_unlink", { desktopDeviceId: phone.desktopDeviceId, mobileDeviceId: phone.mobileDeviceId });
  expect(window.confirm).not.toHaveBeenCalled();
});

it("rejects repeated native actions while busy and reports errors back to AppKit", async () => {
  mockCommands();
  const view = await openNative();
  let reject!: (reason: Error) => void;
  bridge.invoke.mockImplementation((name: string) => name === "mobile_bridge_start_pairing" ? new Promise((_, fail) => { reject = fail; }) : Promise.resolve({}));
  nativeAction(view.panelId, "generate");
  nativeAction(view.panelId, "generate");
  nativeAction(view.panelId, "close");
  await waitFor(() => expect(nativeState().state.busy).toBe(true));
  await waitFor(() => expect(bridge.invoke.mock.calls.filter(([name]) => name === "mobile_bridge_start_pairing")).toHaveLength(1));
  expect(view.onClose).not.toHaveBeenCalled();
  await act(async () => reject(new Error("pairing unavailable")));
  await waitFor(() => expect(nativeState().state.error).toContain("pairing unavailable"));
  expect(nativeState().state.busy).toBe(false);
  nativeAction(view.panelId, "close");
  expect(view.onClose).toHaveBeenCalledOnce();
});

it("refreshes native links and does not fall back to a web pairing dialog on native failure", async () => {
  mockCommands();
  const view = await openNative();
  bridge.invoke.mockClear();
  nativeAction(view.panelId, "refresh");
  await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("mobile_bridge_devices"));
  view.unmount();
  bridge.invoke.mockImplementation(async (name: string) => {
    if (name === "show_macos_mobile_bridge") throw new Error("native unavailable");
    return { devices: [] };
  });
  render(<MobileBridgeWizard accountId="account-1" onClose={vi.fn()} onBound={vi.fn()} onLinksChanged={vi.fn()} />);
  await screen.findByRole("alert");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

it("stores a claimed pairing only in its account and notifies the shared sync controller", async () => {
  mockCommands();
  const view = await openNative();
  vi.useFakeTimers();
  await act(async () => nativeAction(view.panelId, "generate"));
  expect(nativeState().state.pairingUri).not.toBe("");
  bridge.invoke.mockImplementation(async (name: string) => name === "mobile_bridge_pairing_status" ? { status: "claimed", mobileDeviceId: "phone-1" } : name === "mobile_bridge_devices" ? { devices: [] } : {});
  await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
  expect(localStorage.getItem(mobileBridgeStorageKey("account-1"))).toBe(pairing.desktopDeviceId);
  expect(localStorage.getItem(mobileBridgeTokenKey("account-1"))).toBe(pairing.deviceToken);
  expect(localStorage.getItem(mobileBridgeStorageKey("account-2"))).toBeNull();
  expect(view.onBound).toHaveBeenCalledWith(pairing.desktopDeviceId);
});

it("ignores a claimed response arriving after closing the panel", async () => {
  mockCommands();
  const view = await openNative();
  vi.useFakeTimers();
  await act(async () => nativeAction(view.panelId, "generate"));
  expect(nativeState().state.pairingUri).not.toBe("");
  let resolve!: (value: unknown) => void;
  bridge.invoke.mockImplementation((name: string) => name === "mobile_bridge_pairing_status" ? new Promise(done => { resolve = done; }) : Promise.resolve({}));
  await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
  view.unmount();
  await act(async () => resolve({ status: "claimed", mobileDeviceId: "phone-1" }));
  expect(localStorage.getItem(mobileBridgeStorageKey("account-1"))).toBe("desktop-1");
  expect(view.onBound).not.toHaveBeenCalled();
});

it("discards pending pairing state when the account changes", async () => {
  mockCommands();
  const view = await openNative();
  let resolve!: (value: unknown) => void;
  bridge.invoke.mockImplementation((name: string) => name === "mobile_bridge_start_pairing" ? new Promise(done => { resolve = done; }) : Promise.resolve({}));
  nativeAction(view.panelId, "generate");
  await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("mobile_bridge_start_pairing"));
  view.rerender(<MobileBridgeWizard accountId="account-2" onClose={vi.fn()} onBound={vi.fn()} onLinksChanged={vi.fn()} />);
  await act(async () => resolve(pairing));
  expect(nativeState().panelId).not.toBe(view.panelId);
  expect(nativeState().state.pairingUri).toBe("");
  expect(nativeState().state.phones).toEqual([]);
});

it("expires the native QR without storing credentials and permits generating another", async () => {
  mockCommands();
  const view = await openNative();
  vi.useFakeTimers();
  await act(async () => nativeAction(view.panelId, "generate"));
  bridge.invoke.mockImplementation(async (name: string) => name === "mobile_bridge_pairing_status" ? { status: "expired", mobileDeviceId: null } : name === "mobile_bridge_start_pairing" ? pairing : {});
  await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
  expect(nativeState().state.pairingUri).toBe("");
  expect(nativeState().state.status).toContain("已过期");
  expect(localStorage.getItem(mobileBridgeStorageKey("account-1"))).toBe("desktop-1");
  expect(view.onBound).not.toHaveBeenCalled();
  await act(async () => nativeAction(view.panelId, "generate"));
  expect(nativeState().state.pairingUri).not.toBe("");
});

it("does not overlap slow pairing polls and ignores a poll superseded by a new QR", async () => {
  mockCommands();
  const view = await openNative();
  vi.useFakeTimers();
  await act(async () => nativeAction(view.panelId, "generate"));
  let resolve!: (value: unknown) => void;
  bridge.invoke.mockImplementation((name: string) => name === "mobile_bridge_pairing_status" ? new Promise(done => { resolve = done; }) : Promise.resolve(name === "mobile_bridge_start_pairing" ? { ...pairing, pairingId: "pair-2" } : {}));
  await act(async () => { await vi.advanceTimersByTimeAsync(7500); });
  expect(bridge.invoke.mock.calls.filter(([name]) => name === "mobile_bridge_pairing_status")).toHaveLength(1);
  await act(async () => nativeAction(view.panelId, "generate"));
  await act(async () => resolve({ status: "claimed", mobileDeviceId: "phone-1" }));
  expect(nativeState().state.pairingUri).toContain("id=pair-2");
  expect(view.onBound).not.toHaveBeenCalled();
});

it("keeps the Windows web dialog and respects cancelled unlink confirmation", async () => {
  mockCommands();
  vi.mocked(window.confirm).mockReturnValue(false);
  render(<MobileBridgeWizard accountId="account-1" onClose={vi.fn()} onBound={vi.fn()} onLinksChanged={vi.fn()} />);
  await screen.findByText("iPhone");
  expect(screen.getByRole("dialog")).toBeInTheDocument();
  expect(bridge.invoke).not.toHaveBeenCalledWith("show_macos_mobile_bridge", expect.anything());
  fireEvent.click(screen.getByRole("button", { name: "解除绑定" }));
  expect(window.confirm).toHaveBeenCalledOnce();
  expect(bridge.invoke).not.toHaveBeenCalledWith("mobile_bridge_unlink", expect.anything());
});

it("updates native state without reopening the panel when parent callbacks change", async () => {
  mockCommands();
  const view = await openNative();
  const onClose = vi.fn();
  view.rerender(<MobileBridgeWizard accountId="account-1" onClose={onClose} onBound={vi.fn()} onLinksChanged={vi.fn()} />);
  expect(bridge.invoke.mock.calls.filter(([name]) => name === "show_macos_mobile_bridge")).toHaveLength(1);
  nativeAction(view.panelId, "close");
  expect(onClose).toHaveBeenCalledOnce();
  expect(view.onClose).not.toHaveBeenCalled();
});

it("shows every bound phone and removes only the chosen link", async () => {
  let removed = false;
  bridge.invoke.mockImplementation(async (name: string, args?: Record<string, string>) => {
    if (name === "mobile_bridge_devices") return { devices: [
      { desktopDeviceId: "desktop-1", mobileDeviceId: "phone-1", mobileName: "小米 12X" },
      ...(!removed ? [{ desktopDeviceId: "desktop-1", mobileDeviceId: "phone-2", mobileName: "MuMu" }] : []),
    ] };
    if (name === "mobile_bridge_unlink") {
      expect(args).toEqual({ desktopDeviceId: "desktop-1", mobileDeviceId: "phone-2" });
      removed = true;
    }
    return {};
  });
  const onLinksChanged = vi.fn();
  render(<MobileBridgeWizard accountId="account-1" onClose={vi.fn()} onBound={vi.fn()} onLinksChanged={onLinksChanged} />);
  await screen.findByText("小米 12X");
  expect(screen.getByText("MuMu")).toBeInTheDocument();
  fireEvent.click(screen.getAllByRole("button", { name: "解除绑定" })[1]);
  await waitFor(() => expect(onLinksChanged).toHaveBeenCalledOnce());
  expect(screen.getByText("小米 12X")).toBeInTheDocument();
  expect(screen.queryByText("MuMu")).not.toBeInTheDocument();
});
