import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import MobileTaskCard, { type MobileTaskView } from "./MobileTaskCard";

it("shows a shared attachment card and opens it by attachment ID", () => {
  const card: MobileTaskView = {
    taskId: "task-1", initiatingMessageId: "message-1", running: false,
    latest: {
      eventId: "event-2", seq: 2, taskId: "task-1", turnId: null,
      initiatingMessageId: "message-1", kind: "succeeded", summary: "电脑已完成",
      payload: {}, createdAt: "2026-10-13T00:00:00Z",
    },
    steps: [{
      eventId: "event-1", seq: 1, taskId: "task-1", turnId: null,
      initiatingMessageId: "message-1", kind: "artifact_ready", summary: "已生成成果",
      payload: { artifactName: "项目说明.pdf", attachmentId: "attachment-1" },
      createdAt: "2026-10-13T00:00:00Z",
    }],
  };
  const onOpenAttachment = vi.fn();
  render(<MobileTaskCard card={card} attachments={[{
    id: "attachment-1", taskId: "task-1", name: "项目说明.pdf",
    mimeType: "application/pdf", size: 4096,
  }]} onOpenAttachment={onOpenAttachment} />);
  expect(screen.getByText("项目说明.pdf")).toBeInTheDocument();
  expect(screen.getByText(/4 KB/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "在电脑打开" }));
  expect(onOpenAttachment).toHaveBeenCalledWith("attachment-1");
});
