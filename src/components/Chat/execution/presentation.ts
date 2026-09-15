/**
 * 执行步骤的展示投影：工具名、一行摘要、结果预览与敏感信息脱敏。
 *
 * 全部为纯函数，不读取全局状态。后端事件适配层已经做过一次脱敏，
 * 这里再执行一次，保证任何进入 UI 或预览的文本都不携带凭据（规格 §2.3）。
 */
import type { ExecutionStep } from "./types";

/** 步骤一行摘要的最大长度 */
export const MAX_STEP_SUMMARY_CHARS = 120;
/** 步骤结果预览的最大长度 */
export const MAX_RESULT_PREVIEW_CHARS = 600;

const TOOL_DISPLAY_NAMES: Record<string, string> = {
  shell: "执行命令",
  powershell: "执行命令",
  browser: "操作浏览器",
  web_search: "搜索网页",
  web_fetch: "读取网页",
  file_read: "读取文件",
  file_write: "写入文件",
  screen_capture: "截取屏幕",
  harmony_cut: "剪辑媒体",
  office: "生成办公文档",
  artifact_render: "生成成果",
  plan_todo: "更新计划",
  call_fish: "委派子任务",
  chat_ui: "交互卡片",
};

/** 工具名 → 中文动作名；未知工具原样返回。 */
export function toolDisplayName(name: string): string {
  return TOOL_DISPLAY_NAMES[name] ?? name;
}

const AUTHORIZATION_PATTERN = /(authorization\s*[:=]\s*)(?:bearer\s+)?([^\s"',;]+)/gi;
const BEARER_PATTERN = /\bbearer\s+[A-Za-z0-9._~+/=-]{8,}/gi;
const SENSITIVE_KEY_PATTERN =
  /((?:api[_-]?key|access[_-]?token|refresh[_-]?token|auth[_-]?token|client[_-]?secret|password|passwd|secret|token)\s*["']?\s*[:=]\s*["']?)([^\s"',;&)\]}]+)/gi;
const PROVIDER_KEY_PATTERN = /\b(?:sk|ghp|gho|ghs|xox[baprs])-[A-Za-z0-9._-]{8,}\b/g;

const REDACTED = "[已脱敏]";

/** 移除文本中的认证头、密钥键值对与常见供应商 Token。 */
export function redactSensitiveText(text: string): string {
  return text
    .replace(AUTHORIZATION_PATTERN, `$1${REDACTED}`)
    .replace(BEARER_PATTERN, `Bearer ${REDACTED}`)
    .replace(SENSITIVE_KEY_PATTERN, `$1${REDACTED}`)
    .replace(PROVIDER_KEY_PATTERN, REDACTED);
}

/** 按字符边界截断文本，超长时以省略号结尾。 */
export function truncatePreview(text: string, max: number = MAX_RESULT_PREVIEW_CHARS): string {
  if (text.length <= max) return text;
  let end = Math.max(0, max - 1);
  // 不要把代理对切成两半
  if (end > 0 && end < text.length) {
    const code = text.charCodeAt(end - 1);
    if (code >= 0xd800 && code <= 0xdbff) end -= 1;
  }
  return text.slice(0, end) + "…";
}

/** 把工具入参压缩成一行可扫描摘要。 */
export function summarizeToolInput(name: string, input: unknown): string {
  if (input === null || input === undefined) return "";
  if (typeof input !== "object") {
    return redactSensitiveText(truncatePreview(String(input), MAX_STEP_SUMMARY_CHARS));
  }

  const record = input as Record<string, unknown>;
  let raw: string;
  if (name === "browser") {
    const parts = [record["action"]];
    if (record["url"]) parts.push(String(record["url"]).slice(0, 60));
    else if (record["selector"]) parts.push(String(record["selector"]).slice(0, 40));
    raw = parts.filter(Boolean).join(" → ");
  } else if (name === "shell" || name === "powershell") {
    raw = String(record["command"] ?? "");
  } else if (name === "file_read" || name === "file_write") {
    raw = String(record["path"] ?? "");
  } else if (name === "web_search") {
    raw = String(record["query"] ?? "");
  } else if (name === "web_fetch") {
    raw = String(record["url"] ?? "");
  } else if (name === "harmony_cut") {
    raw = [record["action"], record["media_dir"] ?? record["project_id"]].filter(Boolean).join(" ");
  } else if (name === "screen_capture") {
    raw = String(record["mode"] ?? "fullscreen");
  } else {
    raw = Object.entries(record)
      .slice(0, 2)
      .map(([key, value]) => `${key}=${String(value).slice(0, 30)}`)
      .join(" ");
  }
  return redactSensitiveText(truncatePreview(raw, MAX_STEP_SUMMARY_CHARS));
}

/** 从工具结果中取第一行有意义的内容作为摘要。 */
export function summarizeToolResult(result: string): string {
  const firstLine =
    redactSensitiveText(result)
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find((line) => line.length > 0) ?? "";
  return truncatePreview(firstLine, MAX_STEP_SUMMARY_CHARS);
}

/** 步骤状态 → 一行可读文案（不依赖颜色即可理解，规格 §10）。 */
export function describeStep(step: ExecutionStep): string {
  switch (step.state) {
    case "completed":
      return step.summary || `${toolDisplayName(step.name)} 已完成`;
    case "failed":
      return step.summary || "执行失败";
    case "cancelled":
      return step.summary || "已取消";
    case "awaiting_approval":
      return step.summary || "等待批准";
    case "running":
      return step.summary || "正在执行";
    case "pending":
    default:
      return "待处理";
  }
}
