import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Diagnostics from ".";
import { runtimeDiagnostics, type RuntimeDiagnosticsReport } from "../../services/runtimeDiagnostics";

vi.mock("../../services/runtimeDiagnostics", () => ({ runtimeDiagnostics: { check: vi.fn(), repair: vi.fn(), export: vi.fn() } }));

const report: RuntimeDiagnosticsReport = {
  schema_version: 1, checked_at: "2026-10-26T00:00:00Z", platform: "windows", healthy: false,
  items: [{ key: "python:cv2", name: "Python / cv2", status: "broken", path: "C:\\Users\\test\\python.exe", version: null, details: "ModuleNotFoundError: cv2", remediation: "使用完整离线安装包修复 Python。" }],
};

describe("Diagnostics", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(runtimeDiagnostics.check).mockResolvedValue(report);
    vi.mocked(runtimeDiagnostics.repair).mockResolvedValue({ ...report, healthy: true, items: [] });
  });

  it("shows real path, module failure and remediation, then rechecks after offline repair", async () => {
    render(<Diagnostics />);
    expect(await screen.findByText("Python / cv2")).toBeInTheDocument();
    expect(screen.getByText("ModuleNotFoundError: cv2")).toBeInTheDocument();
    expect(screen.getByText(report.items[0].path!)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "离线修复" }));
    expect(await screen.findByText("环境正常")).toBeInTheDocument();
    expect(runtimeDiagnostics.repair).toHaveBeenCalledTimes(1);
  });

  it("keeps the previous result and allows retry after a repair error", async () => {
    vi.mocked(runtimeDiagnostics.repair).mockRejectedValue(new Error("没有经过校验的离线包"));
    render(<Diagnostics />);
    await screen.findByText("Python / cv2");
    fireEvent.click(screen.getByRole("button", { name: "离线修复" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("没有经过校验的离线包");
    expect(screen.getByText("Python / cv2")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新检测" })).toBeEnabled();
  });

  it("handles a cancelled save without reporting success", async () => {
    vi.mocked(runtimeDiagnostics.export).mockResolvedValue(false);
    render(<Diagnostics onClose={vi.fn()} />);
    await screen.findByText("Python / cv2");
    fireEvent.click(screen.getByRole("button", { name: "导出脱敏报告" }));
    expect(await screen.findByText("已取消保存")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "关闭诊断" })).toBeEnabled();
  });

  it("serializes actions and ignores a response after unmount", async () => {
    let resolve!: (value: RuntimeDiagnosticsReport) => void;
    vi.mocked(runtimeDiagnostics.check).mockReturnValue(new Promise((done) => { resolve = done; }));
    const view = render(<Diagnostics />);
    expect(screen.getByRole("button", { name: "离线修复" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "重新检测" })).toBeDisabled();
    view.unmount();
    await act(async () => resolve(report));
    await waitFor(() => expect(runtimeDiagnostics.repair).not.toHaveBeenCalled());
  });
});
