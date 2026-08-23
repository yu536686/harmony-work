import assert from "node:assert/strict";
import test from "node:test";
import { buildSdkRequest } from "./codex-sdk-runner.mjs";

const baseRequest = {
  prompt: "分析报表",
  workingDirectory: "/work",
  model: "deepseek-chat",
  provider: "deepseek",
  providerBaseUrl: "https://api.deepseek.com",
  providerEnvKey: "DEEPSEEK_API_KEY",
};

test("builds a DeepSeek Responses provider without embedding credentials", () => {
  const invocation = buildSdkRequest(baseRequest);
  assert.deepEqual(invocation.codexOptions.config.model_providers.deepseek, {
    name: "deepseek",
    base_url: "https://api.deepseek.com",
    env_key: "DEEPSEEK_API_KEY",
    wire_api: "responses",
  });
  assert.equal(JSON.stringify(invocation).includes("apiKey"), false);
});

test("maps WorkBuddy permissions and attachments to SDK input", () => {
  const invocation = buildSdkRequest({
    ...baseRequest,
    permissionMode: "full",
    attachments: [
      { path: "/work/poster.png", name: "poster.png", mediaType: "image/png" },
      { path: "/work/data.csv", name: "data.csv", mediaType: "text/csv" },
    ],
  });
  assert.equal(invocation.threadOptions.approvalPolicy, "never");
  assert.equal(invocation.threadOptions.sandboxMode, "danger-full-access");
  assert.deepEqual(invocation.input[1], { type: "local_image", path: "/work/poster.png" });
  assert.match(invocation.input[0].text, /data\.csv: \/work\/data\.csv/);
});

test("retains a thread id for resumable server automation", () => {
  const invocation = buildSdkRequest({ ...baseRequest, threadId: "thread-123" });
  assert.equal(invocation.threadId, "thread-123");
  assert.equal(invocation.threadOptions.approvalPolicy, "on-request");
});

test("rejects unsupported permission modes", () => {
  assert.throws(
    () => buildSdkRequest({ ...baseRequest, permissionMode: "admin" }),
    /Unsupported permission mode/,
  );
});
