import { invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { marketingApi, readRememberedMarketingUser } from "../../services/tauri/marketing";

it("forgets the remembered user immediately even when native logout fails offline", async () => {
  localStorage.setItem("workbuddy.marketingUser", JSON.stringify({ id: "A" }));
  let reject!: (reason: unknown) => void;
  vi.mocked(invoke).mockReturnValueOnce(new Promise((_, r) => { reject = r; }));
  const logout = marketingApi.logout();
  expect(readRememberedMarketingUser()).toBeNull();
  reject(new Error("offline"));
  await expect(logout).resolves.toBeUndefined();
  expect(readRememberedMarketingUser()).toBeNull();
});

it("does not restore a remembered account from an in-flight me response after logout", async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockReturnValueOnce(new Promise(r => { resolve = r; }));
  const pending = marketingApi.me();
  vi.mocked(invoke).mockResolvedValueOnce({ ok: true });
  await marketingApi.logout();
  resolve({ user: { id: "old-account" } });
  await expect(pending).rejects.toThrow(/会话/);
  expect(readRememberedMarketingUser()).toBeNull();
});

it("does not restore an in-flight login after logout", async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockReturnValueOnce(new Promise(r => { resolve = r; }));
  const pending = marketingApi.login("a@example.com", "not-a-real-password");
  vi.mocked(invoke).mockResolvedValueOnce({ ok: true });
  await marketingApi.logout();
  resolve({ user: { id: "old-account" } });
  await expect(pending).rejects.toThrow(/会话/);
  expect(readRememberedMarketingUser()).toBeNull();
});
