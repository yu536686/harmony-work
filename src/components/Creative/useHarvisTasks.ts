import { useCallback, useEffect, useState } from "react";
import { loadHarvisTasks, subscribeHarvisTasks, type HarvisTaskKind } from "../../services/tauri/creative";
import { needsHarvisRefresh, refreshHarvisTasks } from "./harvisRefresh";
import { captureMarketingSession, subscribeMarketingSession } from "../../services/tauri/marketing";

export function useHarvisTasks(kind?: HarvisTaskKind, enabled = true, intervalMs = 8_000) {
  const [, setRevision] = useState(0);
  const session = captureMarketingSession();
  const tasks = loadHarvisTasks(session);
  useEffect(() => {
    const sync = () => setRevision(value => value + 1);
    const unsubscribe = subscribeHarvisTasks(sync);
    const unsubscribeSession = subscribeMarketingSession(sync);
    sync();
    return () => { unsubscribe(); unsubscribeSession(); };
  }, []);
  const refresh = useCallback(() => refreshHarvisTasks(loadHarvisTasks(session).filter((task) => !kind || task.kind === kind), session), [kind, session.userId, session.generation]);
  const pendingKeys = tasks.filter((task) => (!kind || task.kind === kind) && needsHarvisRefresh(task)).map((task) => JSON.stringify([task.kind, task.taskId])).sort().join(";");
  useEffect(() => {
    if (!enabled || !pendingKeys) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      await refresh();
      if (!disposed && intervalMs > 0) timer = setTimeout(() => void poll(), intervalMs);
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [enabled, pendingKeys, refresh, intervalMs]);
  return { tasks, refresh, session };
}
