import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { runtimeDiagnostics } from "./runtimeDiagnostics";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("runtimeDiagnostics IPC", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("uses the independently registered diagnostics commands", async () => {
    vi.mocked(invoke).mockResolvedValue({ items: [] });
    await runtimeDiagnostics.check();
    expect(invoke).toHaveBeenCalledWith("check_runtime_diagnostics");
  });

  it("reuses initialization with an explicit offline-only policy then rechecks", async () => {
    vi.mocked(invoke).mockResolvedValue({ items: [] });
    await runtimeDiagnostics.repair();
    expect(invoke).toHaveBeenNthCalledWith(1, "run_environment_bootstrap", { offlineOnly: true });
    expect(invoke).toHaveBeenNthCalledWith(2, "check_runtime_diagnostics");
  });

  it("delegates export to a fresh native redacted report and save dialog, never an arbitrary path", async () => {
    vi.mocked(invoke).mockResolvedValue(false);
    expect(await runtimeDiagnostics.export()).toBe(false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("export_runtime_diagnostics");
  });

  it("does not mask repair failure with a successful check", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("offline package missing"));
    await expect(runtimeDiagnostics.repair()).rejects.toThrow("offline package missing");
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
