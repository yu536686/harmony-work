export type UpdatePhase = "idle" | "checking" | "current" | "available" | "downloading" | "verifying" | "waiting" | "installing" | "error";
export interface UpdateSnapshot {
  phase: UpdatePhase;
  currentVersion: string;
  channel: "stable" | "preview";
  available: { version: string; notes: string; date: string | null; downloadUrl: string } | null;
  downloaded: number;
  total: number | null;
  error: string | null;
  nextCheckAt: number | null;
}
export interface UpdateTransport {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen(handler: (snapshot: UpdateSnapshot) => void): Promise<() => void>;
}
export function createUpdateStore(transport: UpdateTransport) {
  let snapshot: UpdateSnapshot = { phase: "idle", currentVersion: "", channel: "stable", available: null, downloaded: 0, total: null, error: null, nextCheckAt: null };
  const subscribers = new Set<() => void>();
  let connection: Promise<void> | null = null;
  let checking: Promise<UpdateSnapshot> | null = null;
  let installing: Promise<UpdateSnapshot> | null = null;
  let eventRevision = 0;
  function publish(next: UpdateSnapshot) {
    snapshot = next;
    subscribers.forEach((fn) => fn());
  }
  function failure(error: unknown) {
    publish({ ...snapshot, phase: "error", error: error instanceof Error ? error.message : String(error) });
    return snapshot;
  }
  async function command(name: string, args?: Record<string, unknown>) {
    try {
      const result = await transport.invoke<UpdateSnapshot | null>(name, args);
      if (result) publish(result);
      return snapshot;
    } catch (error) { return failure(error); }
  }
  return {
    getSnapshot: () => snapshot,
    subscribe: (fn: () => void) => { subscribers.add(fn); return () => { subscribers.delete(fn); }; },
    connect: () => {
      if (!connection) connection = (async () => {
        await transport.listen((next) => { eventRevision++; publish(next); });
        const revision = eventRevision;
        const result = await transport.invoke<UpdateSnapshot>("update_status");
        if (eventRevision === revision) publish(result);
      })().catch((error) => { connection = null; failure(error); });
      return connection;
    },
    check: () => {
      if (!checking) checking = command("update_check", { manual: true }).finally(() => { checking = null; });
      return checking;
    },
    install: () => {
      if (!installing) installing = command("update_install").finally(() => { installing = null; });
      return installing;
    },
    cancel: () => command("update_cancel"),
    setChannel: (channel: "stable" | "preview") => command("update_set_channel", { channel }),
  };
}
export type UpdateStore = ReturnType<typeof createUpdateStore>;
