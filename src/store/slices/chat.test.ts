import { describe, expect, it } from "vitest";
import { chatSlice } from "./chat";

describe("chat run status lifecycle", () => {
  it("clears the transient runtime phase when a turn stops running", () => {
    let state = chatSlice.reducer(
      undefined,
      chatSlice.actions.setRunStatus({
        sessionId: "session-1",
        status: { iteration: 1, phase: "thinking", detail: "正在规划" },
      }),
    );

    expect(state.runStatusBySession["session-1"]).toBeDefined();

    state = chatSlice.reducer(
      state,
      chatSlice.actions.setRunning({ sessionId: "session-1", running: false }),
    );

    expect(state.runStatusBySession["session-1"]).toBeUndefined();
  });
});
