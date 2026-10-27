import { invoke } from "@tauri-apps/api/core";

export type RuntimeDiagnosticStatus = "ok" | "missing" | "broken" | "timeout" | "warning";

export interface RuntimeDiagnosticItem {
  key: string;
  name: string;
  status: RuntimeDiagnosticStatus;
  path: string | null;
  version: string | null;
  details: string;
  remediation: string;
}

export interface RuntimeDiagnosticsReport {
  schema_version: number;
  checked_at: string;
  platform: string;
  healthy: boolean;
  items: RuntimeDiagnosticItem[];
}

export const runtimeDiagnostics = {
  check: () => invoke<RuntimeDiagnosticsReport>("check_runtime_diagnostics"),
  repair: async (): Promise<RuntimeDiagnosticsReport> => {
    await invoke("run_environment_bootstrap", { offlineOnly: true });
    return runtimeDiagnostics.check();
  },
  export: () => invoke<boolean>("export_runtime_diagnostics"),
};
