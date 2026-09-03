import { pathToFileURL } from "node:url";

const PERMISSIONS = {
  restricted: { approvalPolicy: "untrusted", sandboxMode: "read-only" },
  approval: { approvalPolicy: "on-request", sandboxMode: "workspace-write" },
  full: { approvalPolicy: "never", sandboxMode: "danger-full-access" },
};

export function buildSdkRequest(request) {
  if (!request || typeof request !== "object") {
    throw new TypeError("Codex SDK request must be an object");
  }
  for (const field of ["prompt", "workingDirectory", "model", "provider"]) {
    if (typeof request[field] !== "string" || request[field].trim() === "") {
      throw new TypeError(`Codex SDK request requires ${field}`);
    }
  }

  const permission = PERMISSIONS[request.permissionMode ?? "approval"];
  if (!permission) {
    throw new TypeError(`Unsupported permission mode: ${request.permissionMode}`);
  }

  const provider = request.provider;
  const providerConfig = {
    name: request.providerName ?? provider,
    base_url: request.providerBaseUrl,
    env_key: request.providerEnvKey,
    wire_api: "responses",
  };
  for (const [key, value] of Object.entries(providerConfig)) {
    if (value === undefined) delete providerConfig[key];
  }

  const attachments = Array.isArray(request.attachments) ? request.attachments : [];
  const images = attachments
    .filter((attachment) => attachment?.mediaType?.startsWith("image/") && attachment.path)
    .map((attachment) => ({ type: "local_image", path: attachment.path }));
  const files = attachments
    .filter((attachment) => !attachment?.mediaType?.startsWith("image/") && attachment?.path)
    .map((attachment) => `- ${attachment.name ?? "attachment"}: ${attachment.path}`);
  const prompt = files.length
    ? `${request.prompt}\n\n可访问的附件文件：\n${files.join("\n")}`
    : request.prompt;

  return {
    codexOptions: {
      codexPathOverride: request.codexPath,
      config: {
        model_provider: provider,
        model_providers: { [provider]: providerConfig },
      },
    },
    threadId: typeof request.threadId === "string" && request.threadId ? request.threadId : null,
    threadOptions: {
      model: request.model,
      workingDirectory: request.workingDirectory,
      skipGitRepoCheck: true,
      networkAccessEnabled: request.networkAccessEnabled ?? true,
      ...permission,
    },
    input: images.length ? [{ type: "text", text: prompt }, ...images] : prompt,
  };
}

export async function runSdkRequest(request, writeEvent = console.log) {
  const { Codex } = await import("@openai/codex-sdk");
  const invocation = buildSdkRequest(request);
  const codex = new Codex(invocation.codexOptions);
  const thread = invocation.threadId
    ? codex.resumeThread(invocation.threadId, invocation.threadOptions)
    : codex.startThread(invocation.threadOptions);
  const { events } = await thread.runStreamed(invocation.input);
  for await (const event of events) {
    writeEvent(JSON.stringify(event));
  }
}

async function readStdin() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8").trim();
}

async function main() {
  const raw = await readStdin();
  if (!raw) throw new Error("Codex SDK request was not provided on stdin");
  await runSdkRequest(JSON.parse(raw));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.stack ?? error.message : String(error));
    process.exitCode = 1;
  });
}
