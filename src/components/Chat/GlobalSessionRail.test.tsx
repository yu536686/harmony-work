import { render, screen } from "@testing-library/react";
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
  it("renders new-chat action, filters and sessions in the global sidebar", () => {
    render(
      <GlobalSessionRail
        collapsed={false}
        activeKind="chat"
        counts={{ chat: 1, im: 0, cli: 0 }}
        sessions={sessions}
        activeSessionId="session-1"
        labels={{ chat: "对话", im: "IM", cli: "CLI", conversations: "会话" }}
        onKindChange={vi.fn()}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onNew={vi.fn()}
        t={t}
      />,
    );

    expect(screen.getByRole("button", { name: "chat.newChat" })).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "会话" })).toBeInTheDocument();
    expect(screen.getByText("季度营销复盘")).toBeInTheDocument();
  });

  it("keeps the compact filters available when the application sidebar is collapsed", () => {
    render(
      <GlobalSessionRail
        collapsed
        activeKind="chat"
        counts={{ chat: 1, im: 2, cli: 3 }}
        sessions={sessions}
        activeSessionId="session-1"
        labels={{ chat: "对话", im: "IM", cli: "CLI", conversations: "会话" }}
        onKindChange={vi.fn()}
        onSelect={vi.fn()}
        onDelete={vi.fn()}
        onNew={vi.fn()}
        t={t}
      />,
    );

    expect(screen.queryByText("季度营销复盘")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "对话" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "IM" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "CLI" })).toBeInTheDocument();
  });
});
