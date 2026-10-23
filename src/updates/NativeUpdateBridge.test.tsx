import { act, cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { NativeUpdateBridge, UpdateBadge } from "./UpdateControls";
import { createUpdateStore, type UpdateSnapshot } from "./updateStore";

const { showPanel } = vi.hoisted(() => ({ showPanel: vi.fn() }));
vi.mock("../components/ui/useMacNativeMenu", () => ({ isMacNativeUI: () => true }));
vi.mock("./useMacUpdatePanel", () => ({ useMacUpdatePanel: (...args: unknown[]) => { showPanel(...args); return true; } }));
afterEach(() => { cleanup(); showPanel.mockClear(); });

it("opens the native update window without the main sidebar and avoids duplicate menu listeners", async () => {
  const initial: UpdateSnapshot = { phase: "idle", currentVersion: "1.1.2", channel: "stable", available: null, downloaded: 0, total: null, error: null, nextCheckAt: null };
  const invoke = vi.fn().mockResolvedValue(initial);
  const store = createUpdateStore({ listen: async () => () => {}, invoke });
  const h = render(<NativeUpdateBridge store={store} enabled />);
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("update_status"));
  act(() => window.dispatchEvent(new Event("harmony:native-check-updates")));
  await waitFor(() => expect(showPanel).toHaveBeenCalled());
  expect(invoke.mock.calls.filter(([name]) => name === "update_check")).toHaveLength(1);
  h.rerender(<><NativeUpdateBridge store={store} enabled /><UpdateBadge store={store} enabled /></>);
  act(() => window.dispatchEvent(new Event("harmony:native-check-updates")));
  await waitFor(() => expect(invoke.mock.calls.filter(([name]) => name === "update_check")).toHaveLength(2));
  h.unmount();
  act(() => window.dispatchEvent(new Event("harmony:native-check-updates")));
  expect(invoke.mock.calls.filter(([name]) => name === "update_check")).toHaveLength(2);
});
