import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import GlobalSessionRail from "./GlobalSessionRail";

const sessions = [
  {
    id: "session-1",
    title: "季度营销复盘",
    source: "chat",
    created_at: "2026-08-12T08:00:00Z",
    updated_at: "2026-08-12T08:30:00Z",
    message_count: 6,
    status: "active",
  },
];

const t = (key: string) => key;

describe("GlobalSessionRail", () => {
  it("renders one project tree and selects conversations inside folders", async () => {
    const onSelect = vi.fn();
    render(
      <GlobalSessionRail
        collapsed={false}
        projects={[
          { id: "workspace:c:/harmony", name: "Harmony", path: "C:\\Harmony", kind: "workspace", system: false, sessions },
          { id: "system:im", name: "IM 会话", path: null, kind: "im", system: true, sessions: [] },
          { id: "system:cli", name: "CLI 会话", path: null, kind: "cli", system: true, sessions: [] },
        ]}
        activeProjectId="workspace:c:/harmony"
        activeSessionId="session-1"
        labels={{ projects: "项目", createProject: "新建项目", emptyProject: "暂无对话" }}
        onProjectSelect={vi.fn()}
        onSelect={onSelect}
        onDelete={vi.fn()}
        onRemoveProject={vi.fn()}
        onCreateProject={vi.fn()}
        t={t}
      />,
    );

    expect(screen.getByRole("tree", { name: "项目" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建项目" })).toBeInTheDocument();
    expect(screen.getByText("Harmony")).toBeInTheDocument();
    expect(screen.getByText("季度营销复盘")).toBeInTheDocument();
    await userEvent.click(screen.getByText("季度营销复盘"));
    expect(onSelect).toHaveBeenCalledWith("session-1");
  });

  it("offers a safe remove action only for user projects", async () => {
    const onRemoveProject = vi.fn();
    const project = { id: "workspace:c:/harmony", name: "Harmony", path: "C:\\Harmony", kind: "workspace" as const, system: false, sessions };
    render(
      <GlobalSessionRail
        collapsed={false}
        projects={[
          project,
          { id: "system:im", name: "IM 会话", path: null, kind: "im", system: true, sessions: [] },
        ]}
        activeProjectId={project.id}
        activeSessionId="session-1"
        labels={{ projects: "项目", createProject: "新建项目", emptyProject: "暂无对话", removeProject: "移除项目" }}
        onProjectSelect={vi.fn()}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onRemoveProject={onRemoveProject}
        onCreateProject={vi.fn()}
        t={t}
      />,
    );

    expect(screen.getAllByRole("button", { name: "移除项目" })).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "移除项目" }));
    expect(onRemoveProject).toHaveBeenCalledWith(project);
  });

  it("does not render an unreadable second rail when the application sidebar is collapsed", () => {
    render(
      <GlobalSessionRail
        collapsed
        projects={[
          { id: "workspace:c:/harmony", name: "Harmony", path: "C:\\Harmony", kind: "workspace", system: false, sessions },
        ]}
        activeProjectId="workspace:c:/harmony"
        activeSessionId="session-1"
        labels={{ projects: "项目", createProject: "新建项目", emptyProject: "暂无对话" }}
        onProjectSelect={vi.fn()}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onRemoveProject={vi.fn()}
        onCreateProject={vi.fn()}
        t={t}
      />,
    );

    expect(screen.queryByText("季度营销复盘")).not.toBeInTheDocument();
    expect(screen.queryByRole("tree", { name: "项目" })).not.toBeInTheDocument();
  });
});
