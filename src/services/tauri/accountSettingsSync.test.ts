import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { accountSettingsSyncApi } from "./marketing";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("accountSettingsSyncApi", () => {
  it("uploads only selected cloud settings categories", async () => {
    mockInvoke.mockResolvedValueOnce({ snapshot: null, locallyConfigured: true });

    await accountSettingsSyncApi.backupSelected("user-1", ["aiProvider", "agent"]);

    expect(mockInvoke).toHaveBeenCalledWith("account_settings_sync_backup_selected", {
      request: { userId: "user-1", categoryIds: ["aiProvider", "agent"] },
    });
  });

  it("restores only selected cloud settings categories", async () => {
    mockInvoke.mockResolvedValueOnce({ snapshot: null, locallyConfigured: true });

    await accountSettingsSyncApi.restoreSelected("user-1", ["interactionSafety"]);

    expect(mockInvoke).toHaveBeenCalledWith("account_settings_sync_restore_selected", {
      request: { userId: "user-1", categoryIds: ["interactionSafety"] },
    });
  });
});

