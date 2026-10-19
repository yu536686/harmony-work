import { describe, expect, it } from "vitest";
import { createUpdateStore, type UpdateSnapshot, type UpdateTransport } from "./updateStore";

const initial: UpdateSnapshot = { phase: "idle", currentVersion: "1.1.1", channel: "stable", available: null, downloaded: 0, total: null, error: null, nextCheckAt: null };
const available: UpdateSnapshot = { ...initial, phase: "available", available: { version: "1.1.2-rc.1", notes: "覆盖更新测试", date: null, downloadUrl: "https://download.harmonywatch.cn/releases/test.exe" } };

function harness() {
  let event: (s: UpdateSnapshot) => void = () => {};
  let checks = 0;
  let finish: (s: UpdateSnapshot) => void = () => {};
  const calls: string[] = [];
  const transport: UpdateTransport = {
    listen: async (handler) => { event = handler; return () => {}; },
    invoke: async <T>(command: string) => {
      calls.push(command);
      if (command === "update_check") { checks++; return await new Promise<UpdateSnapshot>((r) => { finish = r; }) as T; }
      return initial as T;
    },
  };
  return { store: createUpdateStore(transport), emit: (s: UpdateSnapshot) => event(s), finish: (s: UpdateSnapshot) => finish(s), checks: () => checks, calls };
}

describe("shared update state", () => {
  it("attaches once and shares one in-flight manual check", async () => {
    const h = harness();
    await Promise.all([h.store.connect(), h.store.connect()]);
    expect(h.calls.filter((c) => c === "update_status")).toHaveLength(1);
    const a = h.store.check(); const b = h.store.check();
    expect(h.checks()).toBe(1);
    h.finish(available); await Promise.all([a, b]);
    expect(h.store.getSnapshot().available?.version).toBe("1.1.2-rc.1");
  });
  it("shares native download progress with every subscriber", async () => {
    const h = harness(); await h.store.connect();
    const seen: number[] = [];
    const unsubscribe = h.store.subscribe(() => seen.push(h.store.getSnapshot().downloaded));
    h.emit({ ...available, phase: "downloading", downloaded: 20, total: 100 });
    expect(seen).toEqual([20]);
    expect(h.store.getSnapshot().total).toBe(100);
    unsubscribe();
  });
  it("retains an available release when a command fails", async () => {
    let emit: (s: UpdateSnapshot) => void = () => {};
    const store = createUpdateStore({ listen: async (fn) => { emit = fn; return () => {}; }, invoke: async <T>(name: string) => { if (name === "update_status") return initial as T; throw new Error("网络中断"); } });
    await store.connect(); emit(available);
    await store.check();
    expect(store.getSnapshot().available?.version).toBe("1.1.2-rc.1");
    expect(store.getSnapshot().error).toContain("网络中断");
  });
});
