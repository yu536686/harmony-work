import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import MobileBridgeWizard, { mobileBridgeStorageKey, mobileBridgeTokenKey } from "./MobileBridgeWizard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem(mobileBridgeStorageKey("account-1"), "desktop-1");
  localStorage.setItem(mobileBridgeTokenKey("account-1"), "T".repeat(43));
  bridge.invoke.mockReset();
  bridge.listen.mockReset();
  bridge.listen.mockResolvedValue(() => {});
  Object.defineProperty(window, "confirm", { configurable: true, value: vi.fn(() => true) });
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
