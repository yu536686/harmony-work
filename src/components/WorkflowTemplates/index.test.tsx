import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import WorkflowTemplates from ".";
import { saveTemplate } from "../../workflows/templates";
import "../../i18n";

beforeEach(() => localStorage.clear());
it("edits, persists and starts a normal draft; failed creation stays recoverable", async () => {
  const use = vi.fn().mockRejectedValue(new Error("离线"));
  const view = render(<WorkflowTemplates onUse={use} />);
  await userEvent.click(screen.getByRole("button", { name: "新建流程" }));
  await userEvent.type(screen.getByLabelText("名称"), "周报");
  await userEvent.type(screen.getByLabelText("任务要求"), "整理本周记录");
  await userEvent.type(screen.getByLabelText("所需技能"), "pptmaster");
  await userEvent.type(screen.getByLabelText("交付成果与验收要求"), "交付可编辑PPT");
  await userEvent.click(screen.getByRole("button", { name: "保存" }));
  view.unmount(); render(<WorkflowTemplates onUse={use} />);
  expect(screen.getByRole("heading", { name: "周报" })).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "开始" }));
  expect(use).toHaveBeenCalledWith(expect.stringMatching(/整理本周记录[\s\S]*pptmaster[\s\S]*交付可编辑PPT/));
  expect(await screen.findByRole("alert")).toHaveTextContent("离线");
});
it("requires confirmation before deleting a stored template", async () => {
  saveTemplate({ name: "报告", prompt: "整理报告", skills: [], deliverables: "" });
  render(<WorkflowTemplates onUse={vi.fn()} />);
  await userEvent.click(screen.getByRole("button", { name: "删除 报告" }));
  expect(screen.getByRole("dialog")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: /^删除$/ }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: "报告" })).not.toBeInTheDocument());
});
