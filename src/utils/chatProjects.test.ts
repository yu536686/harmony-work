import { describe, expect, it } from "vitest";
import type { Session } from "../services/tauri";
import {
  buildChatProjects,
  isLegacyInternalProjectPath,
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
  it("shows only IM and CLI projects before the user explicitly registers a project", () => {
    const projects = buildChatProjects({
      sessions: [],
      defaultWorkspace: "C:\\Harmony",
      projectDirectories: [],
    });

    expect(projects.map((project) => project.name)).toEqual(["IM 会话", "CLI 会话"]);
  });

  it("keeps chats without a registered directory in CLI without exposing runtime folder names", () => {
    const projects = buildChatProjects({
      sessions: [session("default-session", "chat", "C:\\Harmony\\.piscis")],
      defaultWorkspace: "C:\\Harmony",
      projectDirectories: [],
    });

    expect(projects.map((project) => project.name)).toEqual(["IM 会话", "CLI 会话"]);
    expect(projects[1].sessions[0].id).toBe("default-session");
  });

  it("preserves explicitly registered project directories, including empty projects", () => {
    const projects = buildChatProjects({
      sessions: [session("campaign-session", "chat", "C:\\Harmony\\七夕项目")],
      defaultWorkspace: "C:\\Harmony",
      projectDirectories: ["C:\\Harmony\\七夕项目", "D:\\客户项目\\新品项目"],
    });

    expect(projects.map((project) => project.name)).toEqual([
      "七夕项目",
      "新品项目",
      "IM 会话",
      "CLI 会话",
    ]);
    expect(projects[0].sessions.map((item) => item.id)).toEqual(["campaign-session"]);
    expect(projects[1].sessions).toEqual([]);
  });

  it("keeps IM and CLI conversations in fixed system projects", () => {
    const projects = buildChatProjects({
      sessions: [
        session("wecom", "im_wecom"),
        { ...session("cli", "cli"), title: "Piscis CLI" },
      ],
      defaultWorkspace: "C:\\Harmony",
      projectDirectories: [],
    });

    expect(projects.find((project) => project.kind === "im")?.sessions[0].id).toBe("wecom");
    expect(projects.find((project) => project.kind === "cli")?.sessions[0].id).toBe("cli");
  });

  it("deduplicates Windows workspace paths case-insensitively", () => {
    const projects = buildChatProjects({
      sessions: [session("one", "chat", "C:\\Harmony\\Campaign")],
      defaultWorkspace: "C:\\Harmony",
      projectDirectories: ["C:\\Harmony\\Campaign", "c:/harmony/campaign/"],
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

  it("recognizes only legacy internal workspace names during migration", () => {
    expect(isLegacyInternalProjectPath("C:\\Users\\Admin\\Documents\\xiaoheng")).toBe(true);
    expect(isLegacyInternalProjectPath("C:\\Users\\Admin\\Documents\\.piscis")).toBe(true);
    expect(isLegacyInternalProjectPath("C:\\Harmony\\七夕项目")).toBe(false);
  });
});
