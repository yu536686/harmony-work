import { describe, expect, it } from "vitest";
import { isInternalSession } from "./session";

describe("isInternalSession", () => {
  it("hides current Codex heartbeat sessions from the user conversation list", () => {
    expect(isInternalSession({
      id: "workbuddy_heartbeat_global",
      source: "codex_heartbeat_global",
    })).toBe(true);
  });

  it("continues to hide legacy heartbeat sessions after data migration", () => {
    expect(isInternalSession({
      id: "piscis_heartbeat_global",
      source: "piscis_heartbeat_global",
    })).toBe(true);
  });

  it("does not hide ordinary and IM conversations", () => {
    expect(isInternalSession({ id: "chat-1", source: "chat" })).toBe(false);
    expect(isInternalSession({ id: "im-1", source: "im_wecom" })).toBe(false);
  });
});
