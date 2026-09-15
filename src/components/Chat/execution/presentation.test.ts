import { describe, expect, it } from "vitest";
import {
  MAX_RESULT_PREVIEW_CHARS,
  describeStep,
  redactSensitiveText,
  summarizeToolInput,
  toolDisplayName,
  truncatePreview,
} from "./presentation";
import type { ExecutionStep } from "./types";

const step = (overrides: Partial<ExecutionStep> = {}): ExecutionStep => ({
  id: "tool-1",
  kind: "tool",
  name: "shell",
  state: "running",
  summary: "",
  expanded: true,
  ...overrides,
});

describe("工具名称投影", () => {
  it("把常见工具名映射为中文动作", () => {
    expect(toolDisplayName("shell")).toBe("执行命令");
    expect(toolDisplayName("file_read")).toBe("读取文件");
    expect(toolDisplayName("web_search")).toBe("搜索网页");
  });

  it("未知工具名原样返回", () => {
    expect(toolDisplayName("custom_tool")).toBe("custom_tool");
  });
});

describe("工具输入摘要", () => {
  it("命令类工具优先展示命令本身", () => {
    expect(summarizeToolInput("shell", { command: "npm test" })).toBe("npm test");
    expect(summarizeToolInput("powershell", { command: "Get-ChildItem" })).toBe("Get-ChildItem");
  });

  it("文件与网页类工具展示路径或查询词", () => {
    expect(summarizeToolInput("file_read", { path: "/tmp/a.csv" })).toBe("/tmp/a.csv");
    expect(summarizeToolInput("web_search", { query: "Harmony Work" })).toBe("Harmony Work");
    expect(summarizeToolInput("web_fetch", { url: "https://example.com" })).toBe("https://example.com");
  });

  it("未知输入退化为前两个字段的紧凑摘要", () => {
    const summary = summarizeToolInput("custom_tool", { alpha: 1, beta: "two", gamma: 3 });
    expect(summary).toContain("alpha=1");
    expect(summary).toContain("beta=two");
    expect(summary).not.toContain("gamma");
  });

  it("敏感输入在摘要阶段就被脱敏", () => {
    const summary = summarizeToolInput("shell", { command: "curl -H 'Authorization: Bearer abc123456'" });
    expect(summary).not.toContain("abc123456");
  });
});

describe("敏感信息脱敏", () => {
  it("移除 Authorization 头", () => {
    const text = redactSensitiveText('curl -H "Authorization: Bearer abc123456" https://example.com');
    expect(text).not.toContain("abc123456");
    expect(text).toContain("example.com");
  });

  it("移除键值形式的密钥", () => {
    expect(redactSensitiveText("api_key=secret123")).not.toContain("secret123");
    expect(redactSensitiveText('{"password":"p@ss"}')).not.toContain("p@ss");
    expect(redactSensitiveText("token: tok_abcdef")).not.toContain("tok_abcdef");
    expect(redactSensitiveText("OPENAI_API_KEY=sk-live-1234567890")).not.toContain("sk-live-1234567890");
  });

  it("保留普通文本", () => {
    expect(redactSensitiveText("共读取 3 个文件")).toBe("共读取 3 个文件");
  });
});

describe("预览截断", () => {
  it("超长文本被截断并标记", () => {
    const preview = truncatePreview("x".repeat(MAX_RESULT_PREVIEW_CHARS + 100));
    expect(preview.length).toBeLessThanOrEqual(MAX_RESULT_PREVIEW_CHARS);
    expect(preview.endsWith("…")).toBe(true);
  });

  it("短文本保持原样", () => {
    expect(truncatePreview("PASS")).toBe("PASS");
  });

  it("按字符边界截断，不产生半个汉字", () => {
    const preview = truncatePreview("汉".repeat(50), 10);
    expect(preview).toBe("汉".repeat(9) + "…");
  });
});

describe("步骤摘要", () => {
  it("完成态使用已记录的结果摘要", () => {
    expect(describeStep(step({ state: "completed", summary: "3 个文件已更新" }))).toBe("3 个文件已更新");
  });

  it("完成态没有摘要时给出动作描述", () => {
    expect(describeStep(step({ state: "completed" }))).toBe("执行命令 已完成");
  });

  it("各状态都有可读文案", () => {
    expect(describeStep(step({ state: "pending" }))).toBe("待处理");
    expect(describeStep(step({ state: "running" }))).toBe("正在执行");
    expect(describeStep(step({ state: "awaiting_approval" }))).toBe("等待批准");
    expect(describeStep(step({ state: "failed" }))).toBe("执行失败");
    expect(describeStep(step({ state: "cancelled" }))).toBe("已取消");
  });
});
