import { describe, expect, it } from "vitest";
import { mergeMobileTaskEvents, projectMobileTaskCards, type MobileTaskEvent } from "./mobileTaskEvents";

const event = (eventId: string, seq: number, kind: MobileTaskEvent["kind"]): MobileTaskEvent => ({
  eventId, seq, kind, taskId: "task", turnId: "turn", initiatingMessageId: "message",
  summary: kind, payload: {}, createdAt: "2026-10-11T00:00:00.000Z",
});

describe("mobile task progress", () => {
  it("merges replayed and out-of-order events without moving the cursor back", () => {
    const first = mergeMobileTaskEvents([], [event("b", 4, "claimed"), event("a", 2, "queued")], 0);
    expect(first.events.map(item => item.eventId)).toEqual(["a", "b"]);
    expect(first.afterSeq).toBe(4);
    const stale = mergeMobileTaskEvents(first.events, [event("a", 2, "queued")], first.afterSeq);
    expect(stale.events).toHaveLength(2);
    expect(stale.afterSeq).toBe(4);
  });

  it("keeps one terminal card after a retry while the assistant reply remains separate", () => {
    const merged = mergeMobileTaskEvents([event("done", 7, "succeeded")], [event("done", 7, "succeeded")], 7);
    const cards = projectMobileTaskCards(merged.events);
    expect(cards).toHaveLength(1);
    expect(cards[0].running).toBe(false);
    expect(cards[0].initiatingMessageId).toBe("message");
    expect(cards[0].latest.kind).toBe("succeeded");
  });
});
