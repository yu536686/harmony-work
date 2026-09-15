import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { settingsApi, sessionsApi, chatApi, skillsApi, schedulerApi, memoryApi, auditApi } from ".";
import type { AgentEventType } from "./chat";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

// ─── settingsApi ──────────────────────────────────────────────────────────────

describe("settingsApi", () => {
  it("get() calls get_settings", async () => {
    const fakeSettings = { provider: "openai", model: "gpt-4o", language: "zh" };
    mockInvoke.mockResolvedValueOnce(fakeSettings);
    const result = await settingsApi.get();
    expect(mockInvoke).toHaveBeenCalledWith("get_settings");
    expect(result).toEqual(fakeSettings);
  });

  it("save() calls save_settings with updates", async () => {
    const updates = { provider: "anthropic" };
    mockInvoke.mockResolvedValueOnce({ ...updates });
    await settingsApi.save(updates);
    expect(mockInvoke).toHaveBeenCalledWith("save_settings", { updates });
  });

  it("isConfigured() calls is_configured", async () => {
    mockInvoke.mockResolvedValueOnce(true);
    const result = await settingsApi.isConfigured();
    expect(mockInvoke).toHaveBeenCalledWith("is_configured");
    expect(result).toBe(true);
  });
});

// ─── sessionsApi ──────────────────────────────────────────────────────────────

describe("sessionsApi", () => {
  it("list() calls list_sessions with pagination defaults", async () => {
    mockInvoke.mockResolvedValueOnce({ sessions: [], total: 0 });
    await sessionsApi.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_sessions", { limit: 20, offset: 0 });
  });

  it("create() calls create_session with title and optional source", async () => {
    mockInvoke.mockResolvedValueOnce({ id: "abc", title: "Test" });
    await sessionsApi.create("Test", "cli");
    expect(mockInvoke).toHaveBeenCalledWith("create_session", { title: "Test", source: "cli" });
  });

  it("delete() calls delete_session with sessionId", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await sessionsApi.delete("abc");
    expect(mockInvoke).toHaveBeenCalledWith("delete_session", { sessionId: "abc" });
  });

  it("rename() calls rename_session", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await sessionsApi.rename("abc", "New Title");
    expect(mockInvoke).toHaveBeenCalledWith("rename_session", { sessionId: "abc", title: "New Title" });
  });

  it("getExecutionHistory() loads the bounded session event stream", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await chatApi.getExecutionHistory("abc");
    expect(mockInvoke).toHaveBeenCalledWith("get_turn_execution_history", {
      sessionId: "abc",
      limit: 500,
    });
  });
});

// ─── chatApi ──────────────────────────────────────────────────────────────────

describe("chatApi", () => {
  it("getArtifactPreview() sends the session-scoped artifact path", async () => {
    mockInvoke.mockResolvedValueOnce({ kind: "text", uri: "E:/work/result.md" });
    await chatApi.getArtifactPreview("sess1", "E:/work/result.md");
    expect(mockInvoke).toHaveBeenCalledWith("get_artifact_preview", {
      sessionId: "sess1",
      uri: "E:/work/result.md",
    });
  });

  it("saveArtifactCopy() sends an explicit user-selected destination", async () => {
    mockInvoke.mockResolvedValueOnce(128);
    await chatApi.saveArtifactCopy("sess1", "E:/work/result.docx", "D:/exports/result.docx");
    expect(mockInvoke).toHaveBeenCalledWith("save_artifact_copy", {
      sessionId: "sess1",
      uri: "E:/work/result.docx",
      destination: "D:/exports/result.docx",
    });
  });

  it("send() routes the main chat through codex_chat_send", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await chatApi.send("sess1", "hello");
    expect(mockInvoke).toHaveBeenCalledWith("codex_chat_send", {
      sessionId: "sess1",
      content: "hello",
      attachment: null,
      attachments: null,
      explicitSkills: null,
      personaKoiId: null,
      modelOverride: null,
      userContext: null,
      commentContexts: null,
      clearPlan: true,
    });
  });

  it("send() includes structured artifact comments separately from visible text", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    const comments = [{
      id: "annotation-1",
      target: { type: "artifact" as const, id: "artifact-1", title: "方案.pptx", uri: "E:/work/方案.pptx" },
      selection: { type: "point" as const, page: 1, x: 0.3, y: 0.4 },
      comment: "标题缩小",
    }];
    await chatApi.send("sess1", "按批注修改", { commentContexts: comments });
    expect(mockInvoke).toHaveBeenCalledWith("codex_chat_send", expect.objectContaining({
      content: "按批注修改",
      commentContexts: comments,
    }));
  });

  it("cancel() interrupts the active Codex turn by WorkBuddy session id", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await chatApi.cancel("sess1");
    expect(mockInvoke).toHaveBeenCalledWith("codex_chat_cancel", { sessionId: "sess1" });
  });

  it("keeps public execution events on the session-scoped agent channel", async () => {
    const publicEvents: AgentEventType[] = [
      { type: "reasoning_summary_start", id: "reasoning-1", summary_index: 0 },
      {
        type: "reasoning_summary_delta",
        id: "reasoning-1",
        summary_index: 0,
        delta: "正在核对素材",
      },
      { type: "tool_output_delta", id: "command-1", delta: "读取 2 个文件" },
    ];
    const handler = vi.fn();
    const mockListen = vi.mocked(listen);
    mockListen.mockResolvedValueOnce(() => {});

    await chatApi.onEvent("sess1", handler);
    // 不能用 Array.prototype.at：tsconfig 的 lib 是 ES2020，.at 需要 ES2022
    const calls = mockListen.mock.calls;
    const registered = calls[calls.length - 1];
    expect(registered?.[0]).toBe("agent_event_sess1");

    const listener = registered?.[1] as ((event: { payload: AgentEventType }) => void) | undefined;
    publicEvents.forEach((event) => listener?.({ payload: event }));
    expect(handler.mock.calls.map(([event]) => event.type)).toEqual([
      "reasoning_summary_start",
      "reasoning_summary_delta",
      "tool_output_delta",
    ]);
  });
});

// ─── memoryApi ────────────────────────────────────────────────────────────────

describe("memoryApi", () => {
  it("list() calls list_memories", async () => {
    mockInvoke.mockResolvedValueOnce({ memories: [], total: 0 });
    await memoryApi.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_memories");
  });

  it("add() calls add_memory with content", async () => {
    mockInvoke.mockResolvedValueOnce({ id: "m1" });
    await memoryApi.add("remember this");
    expect(mockInvoke).toHaveBeenCalledWith("add_memory", {
      content: "remember this",
      category: undefined,
      confidence: undefined,
    });
  });

  it("delete() calls delete_memory", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await memoryApi.delete("m1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_memory", { memoryId: "m1" });
  });
});

// ─── skillsApi ────────────────────────────────────────────────────────────────

describe("skillsApi", () => {
  it("list() calls list_skills", async () => {
    mockInvoke.mockResolvedValueOnce({ skills: [], total: 0 });
    await skillsApi.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_skills");
  });

  it("install() calls install_skill with source", async () => {
    mockInvoke.mockResolvedValueOnce({ name: "my-skill" });
    await skillsApi.install("https://example.com/SKILL.md");
    expect(mockInvoke).toHaveBeenCalledWith("install_skill", {
      source: "https://example.com/SKILL.md",
    });
  });

  it("uninstall() calls uninstall_skill with skillName", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await skillsApi.uninstall("my-skill");
    expect(mockInvoke).toHaveBeenCalledWith("uninstall_skill", { skillName: "my-skill" });
  });

  it("catalog() calls scan_skill_catalog", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await skillsApi.catalog();
    expect(mockInvoke).toHaveBeenCalledWith("scan_skill_catalog");
  });
});

// ─── schedulerApi ─────────────────────────────────────────────────────────────

describe("schedulerApi", () => {
  it("list() calls list_tasks", async () => {
    mockInvoke.mockResolvedValueOnce({ tasks: [], total: 0 });
    await schedulerApi.list();
    expect(mockInvoke).toHaveBeenCalledWith("list_tasks");
  });

  it("create() calls create_task with params", async () => {
    const params = {
      name: "nightly",
      description: "desc",
      cron_expression: "0 0 * * *",
      task_prompt: "do it",
    };
    mockInvoke.mockResolvedValueOnce({ id: "t1", ...params });
    await schedulerApi.create(params);
    expect(mockInvoke).toHaveBeenCalledWith("create_task", params);
  });

  it("runNow() calls run_task_now with taskId", async () => {
    mockInvoke.mockResolvedValueOnce("ok");
    await schedulerApi.runNow("t1");
    expect(mockInvoke).toHaveBeenCalledWith("run_task_now", { taskId: "t1" });
  });

  it("delete() calls delete_task with taskId", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await schedulerApi.delete("t1");
    expect(mockInvoke).toHaveBeenCalledWith("delete_task", { taskId: "t1" });
  });
});

// ─── auditApi ─────────────────────────────────────────────────────────────────

describe("auditApi", () => {
  it("list() calls get_audit_log with defaults", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await auditApi.list();
    expect(mockInvoke).toHaveBeenCalledWith("get_audit_log", {
      sessionId: undefined,
      toolName: undefined,
      limit: 50,
      offset: 0,
    });
  });

  it("clear() calls clear_audit_log", async () => {
    mockInvoke.mockResolvedValueOnce(null);
    await auditApi.clear("sess1");
    expect(mockInvoke).toHaveBeenCalledWith("clear_audit_log", { sessionId: "sess1" });
  });
});
