export type MobileTaskEventKind = "queued" | "claimed" | "planning" | "tool_started" |
  "tool_finished" | "artifact_ready" | "succeeded" | "failed" | "cancelled";

export type MobileTaskEvent = {
  eventId: string;
  seq: number;
  taskId: string;
  turnId: string | null;
  initiatingMessageId: string | null;
  kind: MobileTaskEventKind;
  summary: string;
  payload: Record<string, string | number>;
  createdAt: string;
};

export function mergeMobileTaskEvents(existing: MobileTaskEvent[], incoming: MobileTaskEvent[], afterSeq: number) {
  const byId = new Map(existing.map(event => [event.eventId, event]));
  for (const event of incoming) if (!byId.has(event.eventId)) byId.set(event.eventId, event);
  return {
    events: [...byId.values()].sort((left, right) => left.seq - right.seq || left.eventId.localeCompare(right.eventId)),
    afterSeq: Math.max(afterSeq, ...incoming.map(event => event.seq)),
  };
}

export function projectMobileTaskCards(events: MobileTaskEvent[]) {
  const tasks = new Map<string, MobileTaskEvent[]>();
  for (const event of events) tasks.set(event.taskId, [...(tasks.get(event.taskId) ?? []), event]);
  return [...tasks.entries()].map(([taskId, related]) => {
    const ordered = related.sort((a, b) => a.seq - b.seq);
    const latest = ordered[ordered.length - 1];
    return {
      taskId,
      initiatingMessageId: latest.initiatingMessageId,
      latest,
      running: !["succeeded", "failed", "cancelled"].includes(latest.kind),
      steps: ordered.filter(event => ["planning", "tool_started", "tool_finished", "artifact_ready"].includes(event.kind)),
    };
  });
}
