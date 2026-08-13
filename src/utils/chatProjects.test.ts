import { describe, expect, it } from "vitest";
import type { Session } from "../services/tauri";
import {
  buildChatProjects,
  joinWorkspacePath,
  projectIdForWorkspace,
} from "./chatProjects";

function session(id: string, source = "chat", workspaceRoot?: string | null): Session {
  return {
    id,
    source,
    title: id,
    status: "active",
    created_at: "2026-08-13T08:00:00Z",
    updated_at: "2026-08-13T08:00:00Z",
    message_count: 1,
    workspace_root: workspaceRoot,
  };
}

describe("chatProjects", () => {
  it("groups chat sessions by effective workspace and preserves empty directories", () => {
    const projects = buildChatProjects({
      sessions: [
        session("default-session"),
        session("campaign-session", "chat", "C:\\Harmony\\七夕项目"),
      ],
      defaultWorkspace: "C:\\Harmony",
      workspaceDirectories: ["C:\\Harmony\\七夕项目", "C:\\Harmony\\新品项目"],
    });

    expect(projects.map((project) => project.name)).toEqual([
      "Harmony",
      "七夕项目",
      "新品项目",
      "IM 会话",
      "CLI 会话",
    ]);
    expect(projects[0].sessions.map((item) => item.id)).toEqual(["default-session"]);
    expect(projects[1].sessions.map((item) => item.id)).toEqual(["campaign-session"]);
    expect(projects[2].sessions).toEqual([]);
  });

  it("keeps IM and CLI conversations in fixed system projects", () => {
    const projects = buildChatProjects({
      sessions: [
        session("wecom", "im_wecom"),
        { ...session("cli", "cli"), title: "Piscis CLI" },
      ],
      defaultWorkspace: "C:\\Harmony",
      workspaceDirectories: [],
    });

    expect(projects.find((project) => project.kind === "im")?.sessions[0].id).toBe("wecom");
    expect(projects.find((project) => project.kind === "cli")?.sessions[0].id).toBe("cli");
  });

  it("deduplicates Windows workspace paths case-insensitively", () => {
    const projects = buildChatProjects({
      sessions: [session("one", "chat", "C:\\Harmony\\Campaign")],
      defaultWorkspace: "C:\\Harmony",
      workspaceDirectories: ["c:/harmony/campaign/"],
    });

    const campaign = projects.filter((project) => project.name.toLowerCase() === "campaign");
    expect(campaign).toHaveLength(1);
    expect(campaign[0].sessions).toHaveLength(1);
    expect(campaign[0].id).toBe(projectIdForWorkspace("C:\\Harmony\\Campaign"));
  });

  it("joins child project names using the workspace path style", () => {
    expect(joinWorkspacePath("C:\\Harmony", "七夕项目")).toBe("C:\\Harmony\\七夕项目");
    expect(joinWorkspacePath("/Users/demo/Harmony", "campaign")).toBe("/Users/demo/Harmony/campaign");
  });
});
