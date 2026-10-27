import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, expect, it, vi } from "vitest";
import UsageBudget from ".";

const overview = (root: string) => ({ workspace_root: root, session_input_tokens: 80, session_output_tokens: 20, project_tokens: 100, session_count: 1, budget: { session_tokens: 0, project_tokens: 0 } });
beforeEach(() => vi.mocked(invoke).mockResolvedValue(overview("/project")));

it("saves actual limits without inventing prices", async () => {
  render(<UsageBudget sessionId="A" />);
  expect(await screen.findByText("80 Token")).toBeVisible();
  const input = screen.getByLabelText("项目 Token 上限");
  await userEvent.clear(input); await userEvent.type(input, "12000");
  await userEvent.click(screen.getByRole("button", { name: "保存限额" }));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("usage_set_budget", { sessionId: "A", budget: { session_tokens: 0, project_tokens: 12000 } }));
});

it("ignores an old session refresh that returns after switching sessions", async () => {
  const pending: { resolve?: (value: unknown) => void } = {};
  const view = render(<UsageBudget sessionId="A" />);
  await screen.findByText("/project");
  vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => { pending.resolve = resolve; }));
  await userEvent.click(screen.getByRole("button", { name: "刷新用量" }));
  vi.mocked(invoke).mockResolvedValue(overview("/new-project"));
  view.rerender(<UsageBudget sessionId="B" />);
  await screen.findByText("/new-project");
  await act(async () => pending.resolve?.(overview("/old-project")));
  expect(screen.queryByText("/old-project")).not.toBeInTheDocument();
  expect(screen.getByText("/new-project")).toBeVisible();
});
