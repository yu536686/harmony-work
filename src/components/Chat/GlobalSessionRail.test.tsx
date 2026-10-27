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
  it("uses a standard navigation row and collapses projects without losing folder state", async () => {
    render(<GlobalSessionRail collapsed={false}
      projects={[{ id: "workspace:test", name: "测试项目", path: "C:\\test", kind: "workspace", system: false, sessions }]}
      activeProjectId="workspace:test" activeSessionId="session-1"
      labels={{ projects: "项目", createProject: "新建项目", emptyProject: "暂无对话" }}
      onProjectSelect={vi.fn()} onSelect={vi.fn()} onDelete={vi.fn()}
      onRemoveProject={vi.fn()} onCreateProject={vi.fn()} t={t} />);
    const toggle = screen.getByRole("button", { name: "项目", expanded: true });
    expect(toggle).toHaveClass("nav-item");
    await userEvent.click(toggle);
    expect(screen.queryByRole("tree", { name: "项目" })).not.toBeInTheDocument();
    expect(document.getElementById(toggle.getAttribute("aria-controls")!)?.inert).toBe(true);
    await userEvent.click(toggle);
    expect(screen.getByRole("tree", { name: "项目" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "季度营销复盘" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /测试项目/ }));
    expect(screen.queryByRole("button", { name: "季度营销复盘" })).not.toBeInTheDocument();
  });

  it("renders one project tree and selects conversations inside folders", async () => {
    const onSelect = vi.fn();
    render(
      <GlobalSessionRail
        collapsed={false}
        projects={[
          { id: "workspace:c:/harmony", name: "Harmony", path: "C:\\Harmony", kind: "workspace", system: false, sessions },
          { id: "system:im", name: "IM 会话", path: null, kind: "im", system: true, sessions: [] },
          { id: "system:unassigned", name: "无项目会话", path: null, kind: "unassigned", system: true, sessions: [] },
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

  it("shows running, unread, and error states without generic chat icons", () => {
    const project = {
      id: "workspace:c:/harmony", name: "Harmony", path: "C:\\Harmony", kind: "workspace" as const,
      system: false,
      sessions: [
        ...sessions,
        { ...sessions[0], id: "session-2", title: "已完成的报告" },
        { ...sessions[0], id: "session-3", title: "失败的报告" },
      ],
    };
    render(<GlobalSessionRail
      collapsed={false}
      projects={[project]}
      activeProjectId={project.id}
      activeSessionId="session-1"
      runningBySession={{ "session-1": true }}
      outcomesBySession={{ "session-2": "unread", "session-3": "error" }}
      labels={{ projects: "项目", createProject: "新建项目", emptyProject: "暂无对话" }}
      onProjectSelect={vi.fn()} onSelect={vi.fn()} onDelete={vi.fn()}
      onRemoveProject={vi.fn()} onCreateProject={vi.fn()} t={t}
    />);
    expect(screen.getByRole("button", { name: "季度营销复盘，正在执行" })).toHaveClass("project-session-select");
    expect(screen.getByRole("button", { name: "已完成的报告，有未读结果" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "失败的报告，执行出错" })).toBeInTheDocument();
    expect(document.querySelectorAll(".project-session-spinner")).toHaveLength(1);
    expect(document.querySelectorAll(".project-session-unread")).toHaveLength(1);
    expect(document.querySelectorAll(".project-session-error")).toHaveLength(1);
  });
});
