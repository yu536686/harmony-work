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

  it("clears stale unread or error status when a new turn starts", () => {
    let state = chatSlice.reducer(undefined, chatSlice.actions.setSessionOutcome({ sessionId: "session-1", outcome: "error" }));
    expect(state.sessionOutcomes["session-1"]).toBe("error");
    state = chatSlice.reducer(state, chatSlice.actions.setRunning({ sessionId: "session-1", running: true }));
    expect(state.sessionOutcomes["session-1"]).toBeUndefined();
  });
});

describe("chat execution model", () => {
  const started = chatSlice.actions.applyExecutionEvent({
    sessionId: "session-1",
    event: { type: "run_status", iteration: 1, phase: "thinking", detail: "正在处理任务" },
  });

  it("applyExecutionEvent 把事件聚合进 executionBySession", () => {
    let state = chatSlice.reducer(undefined, started);
    expect(state.executionBySession["session-1"].state).toBe("running");

    state = chatSlice.reducer(
      state,
      chatSlice.actions.applyExecutionEvent({
        sessionId: "session-1",
        event: { type: "tool_start", id: "tool-1", name: "shell", input: { command: "npm test" } },
      }),
    );

    expect(state.executionBySession["session-1"].steps).toHaveLength(1);
    expect(state.executionBySession["session-1"].steps[0].state).toBe("running");
  });

  it("新一轮 run_status 重置执行模型但不影响历史消息", () => {
    const messages = [
      {
        id: "m1",
        session_id: "session-1",
        role: "user" as const,
        content: "你好",
        created_at: "2026-09-15T10:00:00.000Z",
      },
    ];

    let state = chatSlice.reducer(undefined, chatSlice.actions.setMessages({ sessionId: "session-1", messages }));
    state = chatSlice.reducer(state, started);
    state = chatSlice.reducer(
      state,
      chatSlice.actions.applyExecutionEvent({
        sessionId: "session-1",
        event: { type: "tool_start", id: "tool-1", name: "shell", input: {} },
      }),
    );
    state = chatSlice.reducer(
      state,
      chatSlice.actions.applyExecutionEvent({
        sessionId: "session-1",
        event: { type: "done", total_input_tokens: 1, total_output_tokens: 1 },
      }),
    );
    state = chatSlice.reducer(state, started);

    expect(state.executionBySession["session-1"].steps).toEqual([]);
    expect(state.messagesBySession["session-1"]).toHaveLength(1);
  });

  it("restoreExecutionHistory 写回重建的静态时间线", () => {
    const state = chatSlice.reducer(
      undefined,
      chatSlice.actions.restoreExecutionHistory({
        sessionId: "session-1",
        records: [
          {
            id: "rec-1",
            session_id: "session-1",
            turn_id: "turn-1",
            sequence: 1,
            event_type: "tool_start",
            payload: { type: "tool_start", id: "tool-1", name: "shell", input: {} },
            created_at: "2026-09-15T10:00:00.000Z",
          },
          {
            id: "rec-2",
            session_id: "session-1",
            turn_id: "turn-1",
            sequence: 2,
            event_type: "done",
            payload: { type: "done", total_input_tokens: 1, total_output_tokens: 1 },
            created_at: "2026-09-15T10:00:01.000Z",
          },
        ],
      }),
    );

    const model = state.executionBySession["session-1"];
    expect(model.turnId).toBe("turn-1");
    expect(model.state).toBe("completed");
    expect(model.steps[0].state).toBe("completed");
  });

  it("toggleExecutionStep 切换步骤展开状态", () => {
    let state = chatSlice.reducer(undefined, started);
    state = chatSlice.reducer(
      state,
      chatSlice.actions.applyExecutionEvent({
        sessionId: "session-1",
        event: { type: "tool_start", id: "tool-1", name: "shell", input: {} },
      }),
    );
    state = chatSlice.reducer(
      state,
      chatSlice.actions.applyExecutionEvent({
        sessionId: "session-1",
        event: { type: "tool_end", id: "tool-1", name: "shell", result: "PASS", is_error: false },
      }),
    );
    expect(state.executionBySession["session-1"].steps[0].expanded).toBe(false);

    state = chatSlice.reducer(
      state,
      chatSlice.actions.toggleExecutionStep({ sessionId: "session-1", stepId: "tool-1" }),
    );
    expect(state.executionBySession["session-1"].steps[0].expanded).toBe(true);
  });
});
