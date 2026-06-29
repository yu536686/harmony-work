import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings,
  AppState,
  AttachmentItem,
  ChatCompletionResult,
  ChatMessage,
  ChatStreamEvent,
  ImChannelStatus,
  InstallPayload,
  MarketItem,
  MarketplaceAggregate,
  ModelOption,
  RuntimeTopology,
  SkillItem,
  WeixinLoginPollResult,
  WeixinLoginStartResult,
  WorkContext
} from "../types";
import { defaultState } from "../defaultState";

const STORAGE_KEY = "xiaoheng-workbuddy-state-v2";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function upsertById<T extends { id: string }>(items: T[], incoming: T[]) {
  const map = new Map(items.map((item) => [item.id, item]));
  incoming.forEach((item) => map.set(item.id, { ...map.get(item.id), ...item }));
  return Array.from(map.values());
}

function mergeState(saved: Partial<AppState>): AppState {
  const fallback = structuredClone(defaultState);
  const migratedMessages = saved.messages?.length ? saved.messages : fallback.messages;
  const conversations = saved.conversations?.length
    ? saved.conversations
    : [
        {
          ...fallback.conversations[0],
          messages: migratedMessages,
          updatedAt: new Date().toISOString()
        }
      ];
  const activeConversationId = saved.activeConversationId && conversations.some((item) => item.id === saved.activeConversationId)
    ? saved.activeConversationId
    : conversations[0].id;

  const mergedAgents = saved.agents?.length ? upsertById(fallback.agents, saved.agents) : fallback.agents;
  const mergedSkills = saved.skills?.length ? upsertById(fallback.skills, saved.skills) : fallback.skills;
  const savedSchedules = (saved.schedules ?? []).filter((item) => item.id !== "schedule-daily" && item.id !== "schedule-weekly");
  const mergedSchedules = savedSchedules.length ? upsertById(fallback.schedules, savedSchedules) : fallback.schedules;

  const savedOffices = (saved.offices ?? []).filter((office) => office.id !== "office-product" && office.name !== "产品研发项目组");
  const savedTasks = (saved.tasks ?? []).filter((task) => task.officeId !== "office-product" && !["task-agent-runtime", "task-marketplace", "task-im"].includes(task.id));
  return {
    ...fallback,
    ...saved,
    settings: {
      ...defaultState.settings,
      ...saved.settings,
      availableModels: saved.settings?.availableModels?.length ? saved.settings.availableModels : defaultState.settings.availableModels,
      enabledModels: saved.settings?.enabledModels?.length ? saved.settings.enabledModels : [saved.settings?.model ?? defaultState.settings.model].filter(Boolean),
      heartbeatEnabled: saved.settings?.heartbeatEnabled ?? defaultState.settings.heartbeatEnabled,
      heartbeatIntervalMinutes: saved.settings?.heartbeatIntervalMinutes ?? defaultState.settings.heartbeatIntervalMinutes,
      heartbeatModel: saved.settings?.heartbeatModel ?? defaultState.settings.heartbeatModel,
      heartbeatLastRun: saved.settings?.heartbeatLastRun ?? defaultState.settings.heartbeatLastRun,
      memoryFileEnabled: saved.settings?.memoryFileEnabled ?? defaultState.settings.memoryFileEnabled,
      memoryEmbeddingEnabled: saved.settings?.memoryEmbeddingEnabled ?? defaultState.settings.memoryEmbeddingEnabled,
      memoryEmbeddingModel: saved.settings?.memoryEmbeddingModel ?? defaultState.settings.memoryEmbeddingModel
    },
    agents: mergedAgents,
    offices: savedOffices.length ? savedOffices : structuredClone(defaultState.offices),
    tasks: savedTasks.length ? savedTasks : structuredClone(defaultState.tasks),
    messages: migratedMessages,
    conversations,
    activeConversationId,
    memories: saved.memories?.length ? saved.memories : structuredClone(defaultState.memories),
    skills: mergedSkills,
    schedules: mergedSchedules
  };
}

export async function loadState(): Promise<AppState> {
  if (inTauri()) {
    try {
      const loaded = await invoke<AppState | null>("load_state");
      const state = loaded ? mergeState(loaded) : structuredClone(defaultState);
      const apiKeyConfigured = await hasApiKey(state.settings.provider);
      const installed = await invoke<{ skills: SkillItem[] }>("list_installed_market_skills").catch(() => ({ skills: [] }));
      return {
        ...state,
        settings: { ...state.settings, apiKeyConfigured },
        skills: installed.skills.length ? upsertById(state.skills, installed.skills) : state.skills
      };
    } catch (error) {
      console.error(error);
    }
  }
  const saved = localStorage.getItem(STORAGE_KEY);
  return saved ? mergeState(JSON.parse(saved)) : structuredClone(defaultState);
}

export async function saveState(state: AppState): Promise<void> {
  if (inTauri()) {
    await invoke("save_state", { state });
    return;
  }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export async function startHeartbeatScheduler(): Promise<{ ok: boolean; started: boolean }> {
  if (!inTauri()) return { ok: true, started: false };
  return invoke("start_heartbeat_scheduler");
}

export async function readMemoryFile(): Promise<{ ok: boolean; path: string; byteSize?: number; charCount?: number; content?: string; contentPreview?: string; contentTail?: string; contentTruncated?: boolean }> {
  if (!inTauri()) return { ok: true, path: "preview://MEMORY.md", byteSize: 0, charCount: 0, content: "" };
  return invoke("memory_file_read");
}

export async function searchMemoryFile(settings: AppSettings, query: string, limit = 6): Promise<{ ok: boolean; path: string; query: string; resultCount: number; embeddingEnabled?: boolean; embeddingUsed?: boolean; embeddingModel?: string; results: { score: number; content: string }[] }> {
  if (!inTauri()) return { ok: true, path: "preview://MEMORY.md", query, resultCount: 0, results: [] };
  return invoke("memory_file_search", { query, limit, settings });
}

export async function appendMemoryFile(settings: AppSettings, title: string, content: string): Promise<{ ok: boolean; path: string; byteSize: number; charCount: number; indexOk?: boolean }> {
  if (!inTauri()) return { ok: true, path: "preview://MEMORY.md", byteSize: new Blob([content]).size, charCount: content.length };
  return invoke("memory_file_append", { title, content, settings });
}

export async function registerAttachment(path: string): Promise<AttachmentItem> {
  if (!inTauri()) {
    const name = path.split(/[\\/]/).filter(Boolean).pop() ?? "attachment";
    return {
      id: `preview-${crypto.randomUUID()}`,
      name,
      mime: "application/octet-stream",
      byteSize: 0,
      sha256: "preview",
      originalPath: path,
      cachePath: path,
      createdAt: new Date().toISOString(),
      source: "user-upload"
    };
  }
  return invoke<AttachmentItem>("register_attachment", { path });
}

export async function saveApiKey(apiKey: string, provider: AppSettings["provider"] = "compatible"): Promise<boolean> {
  if (!inTauri()) {
    localStorage.setItem("xiaoheng-preview-api-key", "true");
    return apiKey.trim().length > 0;
  }
  return invoke<boolean>("save_api_key", { apiKey, provider });
}

export async function hasApiKey(provider: AppSettings["provider"]): Promise<boolean> {
  if (!inTauri()) return provider === "ollama" || localStorage.getItem("xiaoheng-preview-api-key") === "true";
  return invoke<boolean>("has_api_key", { provider });
}

export async function validateModelProvider(settings: AppSettings, apiKey?: string): Promise<{ ok: boolean; provider: AppSettings["provider"]; baseUrl: string; models: ModelOption[]; message: string }> {
  if (!inTauri()) {
    const fallback = settings.availableModels?.length ? settings.availableModels : defaultState.settings.availableModels;
    return { ok: true, provider: settings.provider, baseUrl: settings.baseUrl, models: fallback, message: "预览模式：已使用本地模型列表。" };
  }
  return invoke("validate_model_provider", { settings, apiKey });
}

export function buildWorkContext(state: AppState): WorkContext {
  const agentById = new Map(state.agents.map((agent) => [agent.id, agent]));
  const officeById = new Map(state.offices.map((office) => [office.id, office]));
  const activeConversation = state.conversations.find((conversation) => conversation.id === state.activeConversationId);
  const attachments = (activeConversation?.messages ?? [])
    .flatMap((message) => message.attachments ?? [])
    .slice(-30);
  const selectedSkillIds = new Set(activeConversation?.options.skillIds ?? []);
  const permissionMode = activeConversation?.options.permissionMode ?? "approval";
  const enabledSkills = state.skills
    .filter((skill) => skill.enabled && (selectedSkillIds.size === 0 || selectedSkillIds.has(skill.id)))
    .slice(0, 20);
  const selectedOfficeId = activeConversation?.options.officeId;
  const taskMap = new Map<string, AppState["tasks"][number]>();
  state.tasks
    .filter((task) => !task.archived && activeConversation && task.conversationId === activeConversation.id)
    .forEach((task) => taskMap.set(task.id, task));
  if (selectedOfficeId) {
    state.tasks
      .filter((task) => !task.archived && task.officeId === selectedOfficeId)
      .slice(0, 20)
      .forEach((task) => taskMap.set(task.id, task));
  }
  state.tasks
    .filter((task) => !task.archived && ["doing", "review"].includes(task.status))
    .slice(0, 8)
    .forEach((task) => taskMap.set(task.id, task));
  const scopedTasks = Array.from(taskMap.values()).slice(0, 32);
  const scopedOfficeIds = new Set(scopedTasks.map((task) => task.officeId).filter(Boolean));
  if (selectedOfficeId) scopedOfficeIds.add(selectedOfficeId);
  const scopedOffices = state.offices.filter((office) => scopedOfficeIds.size === 0 || scopedOfficeIds.has(office.id)).slice(0, 8);

  const coordinator = activeConversation?.options.expertId
    ? state.agents.find((agent) => agent.id === activeConversation.options.expertId)
    : undefined;

  return {
    method: "openpiscis-compatible-work-pool",
    coordinator: coordinator ?? state.agents.find((agent) => agent.role === "xiaoheng"),
    roster: state.agents,
    offices: scopedOffices.map((office) => ({
      ...office,
      managers: office.managerIds.map((id) => agentById.get(id)).filter(Boolean) as WorkContext["roster"],
      members: office.memberIds.map((id) => agentById.get(id)).filter(Boolean) as WorkContext["roster"],
      tasks: scopedTasks.filter((task) => task.officeId === office.id)
    })),
    tasks: scopedTasks
      .map((task) => ({
        ...task,
        owner: agentById.get(task.ownerId),
        office: officeById.get(task.officeId)
          ? {
              id: officeById.get(task.officeId)!.id,
              name: officeById.get(task.officeId)!.name,
              description: officeById.get(task.officeId)!.description
            }
          : undefined
    })),
    installedSkills: state.skills,
    attachments,
    enabledSkills,
    disabledSkills: state.skills
      .filter((skill) => !skill.enabled)
      .slice(0, 20)
      .map(({ id, name, category }) => ({ id, name, category })),
    activeConversation,
    memories: state.memories.slice(-12),
    schedules: state.schedules.filter((schedule) => schedule.enabled).slice(0, 12),
    settings: {
      ...state.settings,
      sessionPermissionMode: permissionMode,
      allowOutsideWorkspace: permissionMode === "full",
      confirmShell: permissionMode !== "full",
      confirmFileWrite: permissionMode !== "full"
    },
    generatedAt: new Date().toISOString()
  } as WorkContext;
}

export async function completeChat(messages: ChatMessage[], settings: AppSettings, state: AppState): Promise<string> {
  const workContext = buildWorkContext(state);
  if (!inTauri()) {
    await new Promise((resolve) => setTimeout(resolve, 600));
    return `预览模式已读取 ${workContext.roster.length} 位成员、${workContext.enabledSkills.length} 项技能和 ${workContext.tasks.length} 个任务。桌面版会把这些上下文和后端工具一起发送给小亨。`;
  }
  return invoke<string>("complete_chat", { messages, settings, workContext });
}

export async function completeChatStream(
  runId: string,
  messages: ChatMessage[],
  settings: AppSettings,
  state: AppState,
  onEvent: (event: ChatStreamEvent) => void
): Promise<void> {
  const workContext = buildWorkContext(state);
  const useEventChannel = inTauri() && settings.provider !== "anthropic" && (settings.streamOutput || settings.provider === "siliconflow");
  if (!useEventChannel) {
    const answer = await completeChat(messages, settings, state);
    const parsed = parseChatCompletion(answer);
    onEvent({
      type: "done",
      runId,
      content: parsed.content,
      tasks: parsed.tasks,
      todos: parsed.todos,
      toolSteps: parsed.toolSteps
    });
    return;
  }
  const unlisten = await listen<ChatStreamEvent>("xiaoheng-chat-stream", (event) => {
    if (event.payload.runId === runId) onEvent(event.payload);
  });
  try {
    await invoke("complete_chat_stream", { runId, messages, settings, workContext });
  } finally {
    unlisten();
  }
}

export async function stopChatStream(runId: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("stop_chat_stream", { runId });
}

export async function archiveTaskTree(task: unknown): Promise<void> {
  if (!inTauri()) return;
  await invoke("archive_task_tree", { task });
}

const TASK_MARKER_PATTERN = /<!--\s*XIAOHENG_TASK_UPDATES:([\s\S]*?)\s*-->/;
const RUN_MARKER_PATTERN = /<!--\s*XIAOHENG_RUN_TRACE:([\s\S]*?)\s*-->/;
const WORK_POOL_MARKER_PATTERN = /<!--\s*XIAOHENG_WORK_POOL_UPDATES:([\s\S]*?)\s*-->/;

export function parseChatCompletion(answer: string): ChatCompletionResult {
  const taskMatch = answer.match(TASK_MARKER_PATTERN);
  const runMatch = answer.match(RUN_MARKER_PATTERN);
  const workPoolMatch = answer.match(WORK_POOL_MARKER_PATTERN);
  let content = answer.replace(TASK_MARKER_PATTERN, "").replace(RUN_MARKER_PATTERN, "").replace(WORK_POOL_MARKER_PATTERN, "").trim();
  let tasks: ChatCompletionResult["tasks"] = [];
  let workPoolUpdates: ChatCompletionResult["workPoolUpdates"] | undefined;
  let todos: ChatCompletionResult["todos"] = [];
  let toolSteps: ChatCompletionResult["toolSteps"] = [];
  if (taskMatch) {
    try {
      tasks = (JSON.parse(taskMatch[1]) as ChatCompletionResult["tasks"]).filter((item) => item?.id && item?.title && item?.ownerId);
    } catch {
      tasks = [];
    }
  }
  if (runMatch) {
    try {
      const parsed = JSON.parse(runMatch[1]) as Pick<ChatCompletionResult, "todos" | "toolSteps">;
      todos = Array.isArray(parsed.todos) ? parsed.todos : [];
      toolSteps = Array.isArray(parsed.toolSteps) ? parsed.toolSteps : [];
    } catch {
      todos = [];
      toolSteps = [];
    }
  }
  if (workPoolMatch) {
    try {
      workPoolUpdates = JSON.parse(workPoolMatch[1]) as ChatCompletionResult["workPoolUpdates"];
    } catch {
      workPoolUpdates = undefined;
    }
  }
  if (!content) content = "本轮没有生成可显示的文本。";
  return { content, tasks, workPoolUpdates, todos, toolSteps };
}

export async function getRuntimeTopology(): Promise<RuntimeTopology> {
  if (!inTauri()) {
    return {
      product: "小亨 WorkBuddy",
      runtime: "OpenPiscis-compatible preview",
      marketplaceRoot: "Y:/openpisci-0.8.64/marketplace",
      skillMarkets: ["OpenPiscis", "clawhub", "skillhub"],
      modules: [],
      tools: []
    };
  }
  return invoke<RuntimeTopology>("get_runtime_topology");
}

export async function listImChannels(): Promise<ImChannelStatus[]> {
  if (!inTauri()) return [];
  return invoke<ImChannelStatus[]>("list_im_channels");
}

export async function connectImChannel(channelId: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("connect_im_channel", { channelId });
}

export async function installWeixinChannel(force = true): Promise<WeixinLoginStartResult> {
  if (!inTauri()) return { ok: true, status: "starting", message: "预览模式" };
  return invoke<WeixinLoginStartResult>("weixin_login_start", { force });
}

export async function pollWeixinLogin(sessionKey: string): Promise<WeixinLoginPollResult> {
  if (!inTauri()) return { status: "wait", message: "预览模式" };
  return invoke<WeixinLoginPollResult>("weixin_login_poll", { sessionKey });
}

export async function startWeixinMonitor(): Promise<void> {
  if (!inTauri()) return;
  await invoke("weixin_monitor_start");
}

export async function sendWeixinText(toUserId: string, content: string, contextToken?: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("weixin_send_text", { toUserId, content, contextToken });
}

export async function trayNotifyAttention(title: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("tray_notify_attention", { title });
}

export async function disconnectImChannel(channelId: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("disconnect_im_channel", { channelId });
}

export async function saveImChannelConfig(channelId: string, config: Record<string, string | number | boolean>): Promise<void> {
  if (!inTauri()) return;
  await invoke("save_im_channel_config", { request: { channelId, config } });
}

export async function testImChannel(channelId: string, content?: string): Promise<void> {
  if (!inTauri()) return;
  await invoke("test_im_channel", { channelId, content });
}

export async function fetchMarketplace(): Promise<MarketplaceAggregate> {
  if (!inTauri()) {
    return {
      source: "preview",
      experts: [],
      teams: [],
      skills: [],
      skillMarkets: ["OpenPiscis", "clawhub", "skillhub"]
    };
  }
  return invoke<MarketplaceAggregate>("fetch_marketplace_aggregated");
}

export async function searchSkillMarket(source: "harmony" | "clawhub" | "skillhub", query: string, limit = 24, offset = 0): Promise<{ items: MarketItem[]; total: number; source: string; query: string }> {
  if (!inTauri()) return { items: [], total: 0, source, query };
  return invoke("search_skill_market", { request: { source, query, limit, offset } });
}

export async function listInstalledMarketSkills(): Promise<{ skills: SkillItem[] }> {
  if (!inTauri()) return { skills: [] };
  return invoke("list_installed_market_skills");
}

export async function uninstallMarketSkill(source: string, slug: string): Promise<{ ok: boolean; source: string; slug: string }> {
  if (!inTauri()) return { ok: true, source, slug };
  return invoke("uninstall_market_skill", { source, slug });
}

export async function installMarketExpert(item: MarketItem): Promise<InstallPayload> {
  return inTauri() ? invoke<InstallPayload>("install_market_expert", { request: { item } }) : { message: "预览模式", agents: [] };
}

export async function installMarketTeam(item: MarketItem): Promise<InstallPayload> {
  return inTauri() ? invoke<InstallPayload>("install_market_team", { request: { item } }) : { message: "预览模式", agents: [], offices: [] };
}

export async function installMarketSkill(item: MarketItem): Promise<InstallPayload> {
  return inTauri() ? invoke<InstallPayload>("install_market_skill", { request: { item } }) : { message: "预览模式", skills: [] };
}

export async function installRegistrySkill(item: MarketItem): Promise<InstallPayload> {
  const source = item.source === "harmony" ? "harmony" : item.source === "skillhub" ? "skillhub" : "clawhub";
  const slug = item.slug ?? item.id.split("/").pop()?.split("@")[0] ?? item.name;
  return inTauri()
    ? invoke<InstallPayload>("install_registry_skill", {
        request: { source, slug, name: item.name, description: item.description, version: item.version }
      })
    : { message: "预览模式", skills: [] };
}

export async function installSkillFromLocator(locator: string, name?: string): Promise<InstallPayload> {
  return inTauri()
    ? invoke<InstallPayload>("install_skill_from_locator", { request: { locator, name } })
    : { message: "预览模式", skills: [] };
}

export async function installDefaultSkills(): Promise<InstallPayload> {
  return inTauri() ? invoke<InstallPayload>("install_default_skills") : { message: "预览模式", skills: defaultState.skills };
}
