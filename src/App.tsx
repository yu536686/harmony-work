import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { QRCodeSVG } from "qrcode.react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  Activity,
  Archive,
  Bell,
  Bot,
  Boxes,
  Brain,
  BriefcaseBusiness,
  CalendarDays,
  CheckCircle2,
  CircleDot,
  Clock3,
  Download,
  FileText,
  Lightbulb,
  LayoutDashboard,
  Link2,
  LoaderCircle,
  Menu,
  MessageCircle,
  Network,
  PackagePlus,
  Paperclip,
  Plus,
  Save,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Sparkles,
  Square,
  Trash2,
  Users,
  WandSparkles,
  X
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import { defaultState } from "./defaultState";
import {
  appendMemoryFile,
  completeChat,
  completeChatStream,
  connectImChannel,
  disconnectImChannel,
  fetchMarketplace,
  getRuntimeTopology,
  hasApiKey,
  installDefaultSkills,
  installMarketExpert,
  installMarketSkill,
  installMarketTeam,
  installRegistrySkill,
  installSkillFromLocator,
  installWeixinChannel,
  listInstalledMarketSkills,
  listImChannels,
  loadState,
  pollWeixinLogin,
  parseChatCompletion,
  registerAttachment,
  saveApiKey,
  saveImChannelConfig,
  saveState,
  searchSkillMarket,
  sendWeixinText,
  startHeartbeatScheduler,
  startWeixinMonitor,
  archiveTaskTree,
  stopChatStream,
  testImChannel,
  trayNotifyAttention,
  uninstallMarketSkill,
  validateModelProvider
} from "./services/backend";
import type {
  Agent,
  AgentRunTodo,
  AgentToolStep,
  AgentRole,
  AppState,
  AttachmentItem,
  ChatMessage,
  Conversation,
  ImChannelStatus,
  ImSession,
  InstallPayload,
  MarketItem,
  MarketplaceAggregate,
  ModelOption,
  Office,
  RuntimeTopology,
  ScheduleItem,
  SessionPermissionMode,
  SkillItem,
  TaskStatus,
  WorkTask
} from "./types";
import type { WorkPoolUpdates } from "./types";

type Page = "dashboard" | "chat" | "office" | "team" | "tasks" | "skills" | "market" | "im" | "runtime" | "memory" | "schedule" | "settings";
type ModalState =
  | { type: "agent"; item?: Agent }
  | { type: "office"; item?: Office }
  | { type: "task"; item?: WorkTask }
  | { type: "skill"; item?: SkillItem }
  | { type: "schedule"; item?: ScheduleItem };

const nav: { id: Page; label: string; icon: LucideIcon }[] = [
  { id: "dashboard", label: "工作台", icon: LayoutDashboard },
  { id: "chat", label: "对话", icon: MessageCircle },
  { id: "office", label: "项目", icon: BriefcaseBusiness },
  { id: "team", label: "人员", icon: Users },
  { id: "tasks", label: "任务", icon: CheckCircle2 },
  { id: "skills", label: "技能", icon: WandSparkles },
  { id: "market", label: "市场", icon: Boxes },
  { id: "im", label: "IM 连接", icon: Network },
  { id: "runtime", label: "运行时", icon: Activity },
  { id: "memory", label: "记忆", icon: Brain },
  { id: "schedule", label: "自动化", icon: CalendarDays },
  { id: "settings", label: "设置", icon: Settings }
];

const pageMotion = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: { duration: 0.18 }
};

const defaultWorkspaceRoot = "";
const HEARTBEAT_CONVERSATION_ID = "conversation-heartbeat";
const defaultPermissionMode: SessionPermissionMode = "approval";
const permissionModeLabels: Record<SessionPermissionMode, string> = {
  restricted: "受限",
  approval: "请求批准",
  full: "完全访问"
};
const sidebarTips = [
  { title: "快速开始", text: "在对话里输入目标，小亨会选择人员和技能并同步任务中心。" },
  { title: "技能安装", text: "技能页支持启停；市场的已安装页可粘贴本地 SKILL.md 或 URL。" },
  { title: "文件任务", text: "例如抓取新闻生成 Excel，完成后文件会保存到当前会话工作空间并留下任务记录。" },
  { title: "IM 连接", text: "先保存渠道参数，再连接和测试，避免显示没有落地的假连接。" }
];

const providerPresets: Array<{ id: AppState["settings"]["provider"]; label: string; icon: string; baseUrl: string; hint: string }> = [
  { id: "openai", label: "OpenAI", icon: "◎", baseUrl: "https://api.openai.com/v1", hint: "官方 OpenAI API" },
  { id: "deepseek", label: "DeepSeek", icon: "DS", baseUrl: "https://api.deepseek.com/v1", hint: "DeepSeek 官方兼容接口" },
  { id: "siliconflow", label: "硅基流动", icon: "SF", baseUrl: "https://api.siliconflow.cn/v1", hint: "SiliconFlow 模型市场" },
  { id: "zai", label: "智谱/Z.AI", icon: "智", baseUrl: "https://open.bigmodel.cn/api/paas/v4", hint: "GLM / BigModel 兼容接口" },
  { id: "mimo", label: "MiniMax/Mimo", icon: "M", baseUrl: "https://api.mimoai.com/v1", hint: "Mimo 兼容接口，可自定义调整" },
  { id: "qwen", label: "通义千问", icon: "Q", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", hint: "DashScope OpenAI 兼容模式" },
  { id: "doubao", label: "豆包/火山方舟", icon: "豆", baseUrl: "https://ark.cn-beijing.volces.com/api/v3", hint: "火山方舟 OpenAI 兼容接口" },
  { id: "anthropic", label: "Anthropic", icon: "A", baseUrl: "https://api.anthropic.com/v1", hint: "Claude 官方接口" },
  { id: "ollama", label: "Ollama", icon: "O", baseUrl: "http://127.0.0.1:11434/v1", hint: "本地 Ollama 服务" },
  { id: "compatible", label: "自定义兼容", icon: "AI", baseUrl: "", hint: "手动填写 OpenAI 兼容 Base URL" }
];

const modelCategoryLabels: Record<ModelOption["category"], string> = {
  chat: "对话",
  image: "生图",
  video: "视频",
  audio: "语音",
  embedding: "嵌入",
  rerank: "重排序",
  reasoning: "推理",
  coder: "Coder",
  math: "Math",
  vision: "视觉",
  tools: "Tools",
  fim: "FIM",
  moe: "MoE",
  other: "其他"
};

function providerPreset(provider: AppState["settings"]["provider"]) {
  return providerPresets.find((item) => item.id === provider) ?? providerPresets[0];
}

function modelIcon(modelId?: string, models: ModelOption[] = []) {
  if (!modelId) return providerPreset("compatible").icon;
  const model = models.find((item) => item.id === modelId);
  return model?.icon ?? providerPreset(model?.provider ?? "compatible").icon;
}

function availableModelOptions(settings: AppState["settings"]) {
  const options = settings.availableModels?.length ? settings.availableModels : defaultState.settings.availableModels;
  if (settings.model && !options.some((item) => item.id === settings.model)) {
    return [{ id: settings.model, name: settings.model, provider: settings.provider, category: "chat" as const, icon: providerPreset(settings.provider).icon }, ...options];
  }
  return options;
}

function upsertById<T extends { id: string }>(items: T[], incoming: T[]) {
  const map = new Map(items.map((item) => [item.id, item]));
  incoming.forEach((item) => map.set(item.id, { ...map.get(item.id), ...item }));
  return Array.from(map.values());
}

function stripPseudoToolCallText(content: string) {
  let normalized = content
    .replaceAll("<｜｜DSML｜｜", "<｜DSML｜")
    .replaceAll("</｜｜DSML｜｜", "</｜DSML｜")
    .replaceAll("<||DSML||", "<｜DSML｜")
    .replaceAll("</||DSML||", "</｜DSML｜");
  const removeTagged = (value: string, open: string, close: string) => {
    let output = value;
    while (true) {
      const start = output.indexOf(open);
      if (start < 0) break;
      const endRel = output.slice(start).indexOf(close);
      if (endRel < 0) {
        output = output.slice(0, start);
        break;
      }
      output = `${output.slice(0, start)}${output.slice(start + endRel + close.length)}`;
    }
    return output;
  };
  let output = normalized;
  output = removeTagged(output, "<tool_call>", "</tool_call>");
  output = removeTagged(output, "<｜DSML｜tool_calls", "</｜DSML｜tool_calls>");
  output = removeTagged(output, "<｜DSML｜invoke", "</｜DSML｜invoke>");
  return output.trim();
}

function normalizeRunTodos(todos: AgentRunTodo[] | undefined, phase: "done" | "error" | "stopped"): AgentRunTodo[] {
  const fallback: AgentRunTodo[] = phase === "done"
    ? [{ id: "reply", content: "完成本轮分析与回复", status: "completed" }]
    : [{ id: phase, content: phase === "stopped" ? "已停止本轮执行" : "执行中断，等待处理", status: "cancelled" }];
  const source = todos?.length ? todos : fallback;
  return source.map((todo) => {
    if (todo.status === "completed" || todo.status === "cancelled") return todo;
    return { ...todo, status: phase === "done" ? "completed" : "cancelled" };
  });
}

function modelBelongsToProvider(model: ModelOption, provider: AppState["settings"]["provider"]) {
  return provider === "compatible" || model.provider === provider;
}

function selectableChatModels(settings: AppState["settings"]) {
  const options = settings.availableModels?.length ? settings.availableModels : defaultState.settings.availableModels;
  const models = options.filter((model) => (
    (settings.enabledModels.includes(model.id) || model.id === settings.model)
    && modelBelongsToProvider(model, settings.provider)
  ));
  if (models.length) return models;
  if (!settings.availableModels?.length && settings.model) {
    return [{ id: settings.model, name: settings.model, provider: settings.provider, category: "chat" as const, icon: providerPreset(settings.provider).icon }];
  }
  return [];
}

function availableProviderModels(settings: AppState["settings"]) {
  const options = settings.availableModels?.length ? settings.availableModels : defaultState.settings.availableModels;
  return options.filter((model) => modelBelongsToProvider(model, settings.provider));
}

function selectableEmbeddingModels(settings: AppState["settings"]) {
  return availableProviderModels(settings).filter((model) => model.category === "embedding" || /embed|embedding/i.test(`${model.id} ${model.name}`));
}

function resolveChatSettings(settings: AppState["settings"], preferredModel?: string): AppState["settings"] {
  const models = selectableChatModels(settings);
  const usableIds = new Set(models.map((model) => model.id));
  const preferred = preferredModel && usableIds.has(preferredModel) ? preferredModel : undefined;
  const globalDefault = usableIds.has(settings.model) ? settings.model : undefined;
  const fallback = preferred ?? globalDefault ?? models[0]?.id ?? settings.model;
  return { ...settings, model: fallback };
}

function appendAssistantMessage(state: AppState, conversationId: string, messages: ChatMessage[], assistant: ChatMessage) {
  const baseMessages = state.conversations.find((item) => item.id === conversationId)?.messages ?? messages;
  const nextMessages = [...baseMessages, assistant];
  return {
    messages: state.activeConversationId === conversationId ? nextMessages : state.messages,
    conversations: state.conversations.map((item) => item.id === conversationId ? { ...item, messages: nextMessages, updatedAt: new Date().toISOString() } : item)
  };
}

function pauseConversationTasks(tasks: WorkTask[], conversationId: string) {
  return tasks.map((task) => (
    task.conversationId === conversationId && ["todo", "doing", "review"].includes(task.status)
      ? { ...task, status: "paused" as const }
      : task
  ));
}

type RunSnapshot = { runId: string; phase: "idle" | "planning" | "running" | "done" | "error" | "stopped"; todos: AgentRunTodo[]; toolSteps: AgentToolStep[]; message?: string };

type InboundImEvent = {
  channel: ImSession["channel"];
  accountId?: string;
  fromUserId: string;
  messageId?: string | number;
  sessionId?: string;
  contextToken?: string;
  content: string;
  timestamp: string;
};

type SkillInstallProgressEvent = {
  source?: string;
  slug?: string;
  downloaded?: number;
  total?: number;
  percent?: number;
  stage?: string;
  error?: string;
};

type SkillInstallStatus = {
  source?: string;
  slug: string;
  percent: number;
  stage?: string;
  busy: boolean;
  error?: string;
};

async function notifyAgentReply(title: string, body: string) {
  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
    if (granted) {
      sendNotification({ title, body: body.slice(0, 180) });
    }
  } catch {
    // Notification permission can fail in dev/web preview; the reply still remains in the app.
  }
}

function applyInstallPayload(state: AppState, payload: InstallPayload): AppState {
  return {
    ...state,
    agents: payload.agents ? upsertById(state.agents, payload.agents) : state.agents,
    offices: payload.offices ? upsertById(state.offices, payload.offices) : state.offices,
    skills: payload.skills ? upsertById(state.skills, payload.skills) : state.skills
  };
}

function mergeInstalledSkills(state: AppState, skills: SkillItem[]): AppState {
  if (!skills.length) return state;
  return { ...state, skills: upsertById(state.skills, skills) };
}

function applyWorkPoolUpdates(state: AppState, updates?: WorkPoolUpdates): AppState {
  if (!updates) return state;
  const deleteOfficeIds = new Set(updates.deleteOfficeIds ?? []);
  const deleteAgentIds = new Set(updates.deleteAgentIds ?? []);
  const deleteTaskIds = new Set(updates.deleteTaskIds ?? []);
  return {
    ...state,
    agents: upsertById(state.agents.filter((agent) => !deleteAgentIds.has(agent.id)), updates.upsertAgents ?? []),
    offices: upsertById(
      state.offices
        .filter((office) => !deleteOfficeIds.has(office.id))
        .map((office) => ({
          ...office,
          managerIds: office.managerIds.filter((id) => !deleteAgentIds.has(id)),
          memberIds: office.memberIds.filter((id) => !deleteAgentIds.has(id))
        })),
      updates.upsertOffices ?? []
    ),
    tasks: upsertById(
      state.tasks.filter((task) => !deleteTaskIds.has(task.id) && !deleteOfficeIds.has(task.officeId) && !deleteAgentIds.has(task.ownerId)),
      updates.upsertTasks ?? []
    )
  };
}

function avatar(agent?: Agent, size: "sm" | "md" | "lg" | "xl" = "md") {
  return (
    <span className={`avatar avatar-${size}`} style={{ background: agent?.color ?? "#64748b" }}>
      {(agent?.name ?? "?").slice(0, 2)}
    </span>
  );
}

function roleLabel(role: AgentRole) {
  return role === "xiaoheng" ? "小亨" : role === "manager" ? "经理" : "职员";
}

function statusLabel(status: TaskStatus) {
  return { todo: "计划中", doing: "进行中", review: "待验收", done: "已交付", paused: "已暂停", stopped: "已停止" }[status];
}

function priorityLabel(priority: WorkTask["priority"]) {
  return { high: "高", medium: "中", low: "低" }[priority];
}

function dailyScheduleDue(schedule: ScheduleItem, now = new Date()) {
  if (!schedule.enabled || schedule.mode === "manual") return false;
  const time = schedule.time || "04:00";
  const [hour, minute] = time.split(":").map((value) => Number(value));
  const dueAt = new Date(now);
  dueAt.setHours(Number.isFinite(hour) ? hour : 4, Number.isFinite(minute) ? minute : 0, 0, 0);
  const lastRun = schedule.lastRun ? new Date(schedule.lastRun) : undefined;
  return now >= dueAt && (!lastRun || lastRun < dueAt);
}

function App() {
  const [state, setState] = useState<AppState>(structuredClone(defaultState));
  const [ready, setReady] = useState(false);
  const [page, setPage] = useState<Page>("dashboard");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [modal, setModal] = useState<ModalState | null>(null);
  const [notice, setNotice] = useState("");
  const [guidePrompt, setGuidePrompt] = useState("");
  const [tipIndex, setTipIndex] = useState(0);
  const [chatRuns, setChatRuns] = useState<Record<string, RunSnapshot>>({});
  const [skillInstallStatus, setSkillInstallStatus] = useState<Record<string, SkillInstallStatus>>({});
  const stateRef = useRef(state);
  const runsRef = useRef(chatRuns);

  useEffect(() => {
    loadState().then((loaded) => {
      setState(loaded);
      setReady(true);
    });
  }, []);

  useEffect(() => {
    if (ready) void saveState(state);
    stateRef.current = state;
  }, [state, ready]);

  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  useEffect(() => {
    runsRef.current = chatRuns;
  }, [chatRuns]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<SkillInstallProgressEvent>("xiaoheng://skill-install-progress", (event) => {
      if (disposed || !event.payload.slug) return;
      const payload = event.payload;
      const key = `${payload.source ?? "skill"}:${payload.slug}`;
      const percent = Math.max(0, Math.min(100, Math.round(payload.percent ?? 0)));
      setSkillInstallStatus((current) => ({
        ...current,
        [key]: {
          source: payload.source,
          slug: payload.slug!,
          percent,
          stage: payload.stage,
          error: payload.error,
          busy: !payload.error && percent < 100 && payload.stage !== "done"
        }
      }));
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!ready) return;
    void startWeixinMonitor().catch(() => undefined);
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<InboundImEvent>("xiaoheng://im-inbound", (event) => {
      if (disposed || !event.payload.content.trim()) return;
      const inbound = event.payload;
      const now = new Date().toISOString();
      const sessionId = `${inbound.channel}:${inbound.fromUserId}`;
      const current = stateRef.current;
      const existingSession = current.imSessions.find((item) => item.id === sessionId);
      const conversationId = existingSession?.conversationId ?? `im-conversation-${crypto.randomUUID()}`;
      const existingConversation = current.conversations.find((item) => item.id === conversationId);
      const conversation: Conversation = existingConversation ?? {
        id: conversationId,
        title: `微信：${inbound.fromUserId.slice(0, 10)}`,
        createdAt: now,
        updatedAt: now,
        messages: [],
        options: {
          workspaceRoot: current.settings.workspaceRoot || defaultWorkspaceRoot,
          officeId: "",
          expertId: current.agents.find((agent) => agent.role === "xiaoheng")?.id,
          skillIds: current.skills.filter((skill) => skill.enabled).map((skill) => skill.id),
          model: current.settings.model,
          permissionMode: defaultPermissionMode
        }
      };
      const session: ImSession = {
        id: sessionId,
        channel: inbound.channel,
        title: `微信：${inbound.fromUserId.slice(0, 10)}`,
        externalUserId: inbound.fromUserId,
        accountId: inbound.accountId,
        contextToken: inbound.contextToken ?? existingSession?.contextToken,
        conversationId,
        unread: current.activeImSessionId === sessionId ? 0 : (existingSession?.unread ?? 0) + 1,
        lastMessageAt: now
      };
      const nextState: AppState = {
        ...current,
        activeImSessionId: sessionId,
        imSessions: upsertById(current.imSessions, [session]).sort((a, b) => b.lastMessageAt.localeCompare(a.lastMessageAt)),
        conversations: existingConversation ? current.conversations : [conversation, ...current.conversations]
      };
      stateRef.current = nextState;
      setState(nextState);
      setPage("im");
      runConversationAgent({
        state: nextState,
        setState,
        setRuns: setChatRuns,
        conversationId,
        prompt: inbound.content,
        onDone: (content) => sendWeixinText(inbound.fromUserId, content, inbound.contextToken ?? existingSession?.contextToken)
      }).catch((error) => {
        setNotice(`微信消息处理失败：${String(error)}`);
        window.setTimeout(() => setNotice(""), 3000);
      });
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [ready]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<{ command: "new-chat" | "settings" | "show" }>("xiaoheng://tray-command", (event) => {
      if (disposed) return;
      if (event.payload.command === "settings") {
        setPage("settings");
        return;
      }
      if (event.payload.command === "new-chat") {
        const now = new Date().toISOString();
        const workspaceRoot = stateRef.current.settings.workspaceRoot || defaultWorkspaceRoot;
        const enabledSkills = stateRef.current.skills.filter((skill) => skill.enabled);
        const item: Conversation = {
          id: `conversation-${crypto.randomUUID()}`,
          title: "新对话",
          createdAt: now,
          updatedAt: now,
          messages: [],
          options: {
            workspaceRoot,
            officeId: "",
            expertId: stateRef.current.agents.find((agent) => agent.role === "xiaoheng")?.id,
            skillIds: enabledSkills.map((skill) => skill.id),
            model: stateRef.current.settings.model,
            permissionMode: defaultPermissionMode
          }
        };
        setState((current) => ({
          ...current,
          conversations: [item, ...current.conversations],
          activeConversationId: item.id,
          messages: []
        }));
        setPage("chat");
        return;
      }
      setPage("dashboard");
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => setTipIndex((value) => (value + 1) % sidebarTips.length), 4800);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!ready) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const tick = (now = new Date()) => {
      if (disposed) return;
      const current = stateRef.current;
      if (!current.settings.heartbeatEnabled) return;
      const intervalMs = Math.max(5, current.settings.heartbeatIntervalMinutes || 60) * 60 * 1000;
      const last = current.settings.heartbeatLastRun ? Date.parse(current.settings.heartbeatLastRun) : 0;
      if (now.getTime() - last < intervalMs) return;
      const busy = Object.values(runsRef.current).some((run) => run.phase === "planning" || run.phase === "running");
      if (busy) return;
      const nowIso = now.toISOString();
      const conversationId = HEARTBEAT_CONVERSATION_ID;
      const existing = current.conversations.find((item) => item.id === conversationId);
      const xiaohengId = current.agents.find((agent) => agent.role === "xiaoheng")?.id;
      const conversation: Conversation = existing ?? {
        id: conversationId,
        title: "Heartbeat 心跳",
        createdAt: nowIso,
        updatedAt: nowIso,
        messages: [],
        options: {
          workspaceRoot: current.settings.workspaceRoot || defaultWorkspaceRoot,
          officeId: "",
          expertId: xiaohengId,
          skillIds: current.skills.filter((skill) => skill.enabled).map((skill) => skill.id),
          model: current.settings.heartbeatModel || current.settings.model,
          permissionMode: "restricted"
        }
      };
      const dueSchedules = current.schedules.filter((schedule) => dailyScheduleDue(schedule, now));
      if (!dueSchedules.length) {
        setState((value) => ({ ...value, settings: { ...value.settings, heartbeatLastRun: nowIso } }));
        return;
      }
      const due = dueSchedules[0];
      const prompt = due.action === "dream_memory"
        ? [
            "这是 WorkBuddy Heartbeat 触发的低 token 自动化：整理长期记忆。",
            "请只处理本次自动化，不要巡检任务中心、暂停任务或 IM。",
            "请基于最近对话、已有记忆和明确长期有效事实，调用 memory_search 检索相关旧记忆，再调用 memory_append 追加简洁的长期记忆条目。",
            "只保存长期有效的用户偏好、项目事实、决策、承诺和复用规则；不要保存流水账。"
          ].join("\n")
        : [
            "这是 WorkBuddy Heartbeat 触发的低 token 自动化。",
            due.prompt || "请执行当前自动化，并在必要时更新任务或记忆。",
            "不要额外巡检任务中心、暂停任务或 IM。"
          ].join("\n");
      const nextState: AppState = {
        ...current,
        conversations: existing ? current.conversations : [conversation, ...current.conversations],
        settings: { ...current.settings, heartbeatLastRun: nowIso },
        schedules: current.schedules.map((schedule) => schedule.id === due.id ? { ...schedule, lastRun: nowIso } : schedule)
      };
      setState(nextState);
      void runConversationAgent({
        state: nextState,
        setState,
        setRuns: setChatRuns,
        conversationId,
        prompt
      }).catch((error) => console.error("heartbeat failed", error));
    };
    void startHeartbeatScheduler().catch((error) => console.error("heartbeat scheduler failed", error));
    listen<{ epochSeconds?: number }>("xiaoheng://heartbeat-tick", (event) => {
      const epoch = event.payload.epochSeconds;
      tick(epoch ? new Date(epoch * 1000) : new Date());
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    tick();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [ready]);

  const xiaoheng = state.agents.find((agent) => agent.role === "xiaoheng");
  const activeTasks = state.tasks.filter((task) => task.status !== "done").length;
  const donePercent = Math.round((state.tasks.filter((task) => task.status === "done").length / Math.max(1, state.tasks.length)) * 100);

  const mergeInstall = (payload: InstallPayload) => {
    setState((current) => applyInstallPayload(current, payload));
    setNotice(payload.message);
    window.setTimeout(() => setNotice(""), 2200);
  };

  const retryTask = (task: WorkTask) => {
    const owner = state.agents.find((agent) => agent.id === task.ownerId);
    const prompt = [
      `请重试任务「${task.title}」。`,
      `负责人：${owner?.name ?? task.ownerId}。`,
      `原任务说明：${task.description}`,
      "请从暂停/停止原因继续执行，必要时重新调用工具，并实时同步任务中心状态。"
    ].join("\n");
    setState((current) => ({
      ...current,
      activeConversationId: task.conversationId ?? current.activeConversationId,
      messages: current.conversations.find((item) => item.id === (task.conversationId ?? current.activeConversationId))?.messages ?? current.messages,
      tasks: current.tasks.map((item) => item.id === task.id ? { ...item, status: "todo" } : item)
    }));
    setGuidePrompt(prompt);
    setPage("chat");
  };

  const content = () => {
    switch (page) {
      case "chat":
        return <ChatPage state={state} setState={setState} guidePrompt={guidePrompt} clearGuidePrompt={() => setGuidePrompt("")} runs={chatRuns} setRuns={setChatRuns} />;
      case "office":
        return <OfficePage state={state} setState={setState} openModal={setModal} />;
      case "team":
        return <TeamPage state={state} setState={setState} openModal={setModal} />;
      case "tasks":
        return <TasksPage state={state} setState={setState} openModal={setModal} onRetry={retryTask} />;
      case "skills":
        return <SkillsPage state={state} setState={setState} openModal={setModal} onInstall={mergeInstall} />;
      case "market":
        return <MarketplacePage state={state} setState={setState} openModal={setModal} onInstall={mergeInstall} installStatus={skillInstallStatus} setInstallStatus={setSkillInstallStatus} />;
      case "im":
        return <ImPage state={state} setState={setState} runs={chatRuns} setRuns={setChatRuns} />;
      case "runtime":
        return <RuntimePage />;
      case "memory":
        return <MemoryPage state={state} setState={setState} />;
      case "schedule":
        return <SchedulePage state={state} setState={setState} openModal={setModal} />;
      case "settings":
        return <SettingsPage state={state} setState={setState} />;
      default:
        return <Dashboard state={state} xiaoheng={xiaoheng} activeTasks={activeTasks} donePercent={donePercent} navigate={setPage} />;
    }
  };

  return (
    <div className={`app-shell ${sidebarOpen ? "sidebar-open" : "sidebar-closed"}`}>
      <aside className="sidebar">
        <div className="brand">
          <img src="/logo.png" alt="小亨 WorkBuddy" />
          <div>
            <strong>小亨</strong>
            <span>WorkBuddy</span>
          </div>
        </div>
        <nav>
          {nav.map((item) => (
            <button key={item.id} className={page === item.id ? "active" : ""} onClick={() => setPage(item.id)} title={item.label}>
              <item.icon size={18} />
              <span>{item.label}</span>
              {item.id === "tasks" && activeTasks > 0 && <em>{activeTasks}</em>}
            </button>
          ))}
        </nav>
        <button className="guide-entry" onClick={() => { setGuidePrompt("你好，我是新用户，请用简短的方式介绍一下小亨能做什么，以及我该如何开始？"); setPage("chat"); }}>
          <FileText size={17} />
          <span>新手指引</span>
        </button>
        <div className="sidebar-callout">
          <Sparkles size={18} />
          <strong>{sidebarTips[tipIndex].title}</strong>
          <p>{sidebarTips[tipIndex].text}</p>
          <div className="tip-dots">
            {sidebarTips.map((tip, index) => <button key={tip.title} className={index === tipIndex ? "active" : ""} onClick={() => setTipIndex(index)} title={tip.title} />)}
          </div>
        </div>
        <div className="sidebar-footer">
          {avatar(xiaoheng, "sm")}
          <div>
            <strong>小亨</strong>
            <span>在线协调</span>
          </div>
        </div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <button className="icon-btn" onClick={() => setSidebarOpen(!sidebarOpen)} title="切换导航">
            <Menu size={20} />
          </button>
          <div className="searchbox">
            <Search size={17} />
            <input placeholder="搜索任务、成员、技能或市场条目" />
          </div>
          <div className="top-actions">
            <span className="secure">
              <ShieldCheck size={15} />
              本地优先
            </span>
            <button className="icon-btn" title="通知">
              <Bell size={18} />
            </button>
            {avatar(xiaoheng, "sm")}
          </div>
        </header>
        <div className="page-scroll">
          <AnimatePresence mode="wait">
            <motion.div key={page} className={page === "chat" ? "full-bleed-page" : undefined} {...pageMotion}>
              {content()}
            </motion.div>
          </AnimatePresence>
        </div>
      </main>

      <AnimatePresence>{modal && <EditorModal modal={modal} state={state} setState={setState} close={() => setModal(null)} />}</AnimatePresence>
      <AnimatePresence>{notice && <motion.div className="toast" initial={{ opacity: 0, y: 18 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 18 }}>{notice}</motion.div>}</AnimatePresence>
      {!ready && (
        <div className="loading-screen">
          <LoaderCircle className="spin" />
          <span>正在初始化小亨运行时</span>
        </div>
      )}
    </div>
  );
}

function PageHeader({ eyebrow, title, description, action }: { eyebrow: string; title: string; description: string; action?: React.ReactNode }) {
  return (
    <div className="page-header">
      <div>
        <span className="eyebrow">{eyebrow}</span>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {action}
    </div>
  );
}

function isApprovalRequired(content: string) {
  return content.includes("APPROVAL_REQUIRED");
}

function confirmSessionFullAccess(conversationTitle: string) {
  return window.confirm(`小亨需要在当前会话执行 Shell、写入文件或访问工作区外路径。\n\n是否仅为「${conversationTitle}」授予完全访问权限，并自动继续执行？`);
}

function formatByteSize(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function attachmentSummary(attachments: AttachmentItem[]) {
  if (!attachments.length) return "";
  return [
    "",
    "[用户上传附件]",
    "这些文件已复制到 WorkBuddy 附件缓存；需要读取内容时请优先调用 read_attachment_text，并使用 attachmentId。不要把附件当作未上传或要求用户重新发送。",
    ...attachments.map((item, index) => `${index + 1}. ${item.name}\n- attachmentId: ${item.id}\n- mime: ${item.mime}\n- byteSize: ${item.byteSize}\n- cachePath: ${item.cachePath}`)
  ].join("\n");
}

function AttachmentCards({ attachments }: { attachments?: AttachmentItem[] }) {
  if (!attachments?.length) return null;
  const listStyle: React.CSSProperties = { display: "grid", gap: 8, marginTop: 10 };
  const cardStyle: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10, maxWidth: 420, padding: "9px 10px", border: "1px solid rgba(148, 163, 184, 0.35)", borderRadius: 8, background: "rgba(15, 23, 42, 0.04)" };
  const metaStyle: React.CSSProperties = { display: "block", opacity: 0.72, marginTop: 2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 340 };
  return (
    <div className="attachment-cards" style={listStyle}>
      {attachments.map((item) => (
        <div className="attachment-card" key={item.id} title={item.cachePath} style={cardStyle}>
          <FileText size={16} />
          <div>
            <strong>{item.name}</strong>
            <small style={metaStyle}>{item.mime || "application/octet-stream"} · {formatByteSize(item.byteSize)}</small>
          </div>
        </div>
      ))}
    </div>
  );
}

function GeneratedPathCards({ content }: { content: string }) {
  const matches = Array.from(new Set(content.match(/[A-Za-z]:\\[^\n\r"'<>|]+?\.(?:pptx|xlsx|docx|pdf|zip|png|jpg|jpeg|webp|txt|md|csv|json|html|py|ps1|exe)/gi) ?? []));
  if (!matches.length) return null;
  const listStyle: React.CSSProperties = { display: "grid", gap: 8, marginTop: 10 };
  const cardStyle: React.CSSProperties = { display: "flex", alignItems: "center", gap: 10, maxWidth: 520, padding: "9px 10px", border: "1px solid rgba(14, 165, 233, 0.35)", borderRadius: 8, background: "rgba(14, 165, 233, 0.06)" };
  const metaStyle: React.CSSProperties = { display: "block", opacity: 0.72, marginTop: 2, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 440 };
  return (
    <div className="attachment-cards" style={listStyle}>
      {matches.slice(0, 4).map((path) => (
        <div className="attachment-card" key={path} title={path} style={cardStyle}>
          <FileText size={16} />
          <div>
            <strong>{path.split("\\").pop()}</strong>
            <small style={metaStyle}>{path}</small>
          </div>
        </div>
      ))}
    </div>
  );
}

function isContinueInput(text: string) {
  return ["继续", "继续执行", "继续吧", "接着来", "接着做", "继续上一步", "continue"].includes(text.trim().toLowerCase());
}

function isMetaErrorInput(text: string) {
  const compact = text.trim().toLowerCase();
  return (compact.includes("报错") || compact.includes("错误") || compact.includes("error"))
    && (compact.includes("分析") || compact.includes("原因") || compact.includes("为什么") || compact.includes("怎么回事"));
}

function resolvePromptWithPendingObjective(rawPrompt: string, conversation: Conversation) {
  const prompt = rawPrompt.trim();
  if (isContinueInput(prompt) && conversation.options.pendingObjective?.trim()) {
    return `继续执行当前未完成任务：\n${conversation.options.pendingObjective.trim()}`;
  }
  return prompt;
}

function nextPendingObjective(prompt: string, conversation: Conversation) {
  if (isContinueInput(prompt) || isMetaErrorInput(prompt)) return conversation.options.pendingObjective;
  return prompt.trim();
}

function withFullAccessConversation(state: AppState, conversationId: string, messages: ChatMessage[]): AppState {
  return {
    ...state,
    messages: state.activeConversationId === conversationId ? messages : state.messages,
    activeConversationId: conversationId,
    conversations: state.conversations.map((item) => item.id === conversationId
      ? { ...item, messages, options: { ...item.options, permissionMode: "full" }, updatedAt: new Date().toISOString() }
      : item)
  };
}

async function runConversationAgent({
  state,
  setState,
  setRuns,
  conversationId,
  prompt,
  onDone
}: {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  setRuns: React.Dispatch<React.SetStateAction<Record<string, RunSnapshot>>>;
  conversationId: string;
  prompt: string;
  onDone?: (content: string) => void | Promise<void>;
}) {
  const conversation = state.conversations.find((item) => item.id === conversationId);
  if (!conversation || !prompt.trim()) return;
  const effectivePrompt = resolvePromptWithPendingObjective(prompt, conversation);
  const pendingObjective = nextPendingObjective(prompt, conversation);
  const runId = `run-${crypto.randomUUID()}`;
  const userMessage: ChatMessage = { id: crypto.randomUUID(), role: "user", content: effectivePrompt, timestamp: new Date().toISOString() };
  const messages = [...conversation.messages, userMessage];
  const title = conversation.title === "新对话" || conversation.title.startsWith("微信：") ? conversation.title : conversation.title;
  setState((current) => ({
    ...current,
    messages: current.activeConversationId === conversation.id ? messages : current.messages,
    conversations: current.conversations.map((item) => item.id === conversation.id ? { ...item, title: title === "新对话" ? effectivePrompt.slice(0, 24) : title, messages, options: { ...item.options, pendingObjective }, updatedAt: new Date().toISOString() } : item)
  }));
  setRuns((current) => ({
    ...current,
    [conversation.id]: {
      runId,
      phase: "running",
      message: "小亨 Gateway 正在处理请求。",
      todos: [],
      toolSteps: []
    }
  }));
  const stateForChat: AppState = {
    ...state,
    messages,
    activeConversationId: conversation.id,
    conversations: state.conversations.map((item) => item.id === conversation.id ? { ...item, messages, options: { ...item.options, pendingObjective } } : item),
    settings: resolveChatSettings({ ...state.settings, workspaceRoot: conversation.options.workspaceRoot }, conversation.options.model || state.agents.find((agent) => agent.id === conversation.options.expertId)?.defaultModel || state.settings.model)
  };
  let streamedContent = "";
  let streamErrorHandled = false;
  await completeChatStream(runId, messages, stateForChat.settings, stateForChat, (event) => {
    if (event.type === "started") {
      setRuns((current) => ({ ...current, [conversation.id]: { runId: event.runId, phase: "running", message: event.message ?? "小亨正在执行。", todos: current[conversation.id]?.todos ?? [], toolSteps: current[conversation.id]?.toolSteps ?? [] } }));
    }
    if (event.type === "todo") {
      setRuns((current) => ({ ...current, [conversation.id]: { ...(current[conversation.id] ?? { runId: event.runId, phase: "running", toolSteps: [] }), phase: "running", todos: event.todos } }));
    }
    if (event.type === "token") {
      streamedContent += event.content;
      setRuns((current) => ({ ...current, [conversation.id]: { ...(current[conversation.id] ?? { runId: event.runId, phase: "running", todos: [], toolSteps: [] }), phase: "running", message: "正在流式生成回复..." } }));
    }
    if (event.type === "content_reset") {
      streamedContent = event.content ?? "";
    }
    if (event.type === "assistant_note") {
      setRuns((current) => {
        const run = current[conversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
        return { ...current, [conversation.id]: { ...run, phase: "running", message: event.content } };
      });
    }
    if (event.type === "tool_start") {
      setRuns((current) => {
        const run = current[conversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
        return { ...current, [conversation.id]: { ...run, phase: "running", message: `正在调用工具：${event.step.name}`, toolSteps: upsertById(run.toolSteps, [event.step]) } };
      });
    }
    if (event.type === "tool_end") {
      setRuns((current) => {
        const run = current[conversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
        return { ...current, [conversation.id]: { ...run, phase: "running", message: `工具完成：${event.step.name}`, todos: event.todos?.length ? event.todos : run.todos, toolSteps: upsertById(run.toolSteps, [event.step]) } };
      });
      if (event.tasks?.length || event.workPoolUpdates) {
        setState((current) => {
          const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks!) } : current;
          return applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
        });
      }
    }
    if (event.type === "done") {
      const content = stripPseudoToolCallText(event.content) || stripPseudoToolCallText(streamedContent) || "本轮没有生成可显示的文本。";
      const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
      setRuns((current) => ({
        ...current,
        [conversation.id]: {
          runId: event.runId,
          phase: "done",
          message: (event.toolSteps.length || current[conversation.id]?.toolSteps.length) ? "本轮执行完成，工具结果和 todo 已保留在这里。" : "本轮回复完成。",
          todos: normalizeRunTodos(event.todos.length ? event.todos : current[conversation.id]?.todos, "done"),
          toolSteps: event.toolSteps.length ? event.toolSteps : (current[conversation.id]?.toolSteps ?? [])
        }
      }));
      setState((current) => {
        const nextMessages = [...(current.conversations.find((item) => item.id === conversation.id)?.messages ?? messages), assistant];
        return {
          ...current,
          ...applyWorkPoolUpdates({
            ...current,
            tasks: event.tasks.length ? upsertById(current.tasks, event.tasks.map((task) => task.status === "done" && !task.deliveredAt ? { ...task, deliveredAt: new Date().toISOString() } : task)) : current.tasks
          }, event.workPoolUpdates),
          messages: current.activeConversationId === conversation.id ? nextMessages : current.messages,
          conversations: current.conversations.map((item) => item.id === conversation.id ? { ...item, messages: nextMessages, options: { ...item.options, pendingObjective: undefined }, updatedAt: new Date().toISOString() } : item)
        };
      });
      if (content.trim() !== "HEARTBEAT_OK") {
        void notifyAgentReply(conversation.title.startsWith("微信：") ? "微信会话有新回复" : "小亨已完成回复", content);
        void trayNotifyAttention(conversation.title);
      }
      void onDone?.(content);
    }
    if (event.type === "stopped") {
      const content = event.content?.trim() || "已停止本轮执行。";
      const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
      setRuns((current) => ({ ...current, [conversation.id]: { runId: event.runId, phase: "stopped", message: "用户已停止本轮 Agent 执行。", todos: normalizeRunTodos(event.todos, "stopped"), toolSteps: event.toolSteps ?? current[conversation.id]?.toolSteps ?? [] } }));
      setState((current) => {
        const nextMessages = [...(current.conversations.find((item) => item.id === conversation.id)?.messages ?? messages), assistant];
        return {
          ...current,
          tasks: current.tasks.map((task) => task.conversationId === conversation.id && ["todo", "doing", "review", "paused"].includes(task.status) ? { ...task, status: "stopped" } : task),
          messages: current.activeConversationId === conversation.id ? nextMessages : current.messages,
          conversations: current.conversations.map((item) => item.id === conversation.id ? { ...item, messages: nextMessages, updatedAt: new Date().toISOString() } : item)
        };
      });
      void onDone?.(content);
    }
    if (event.type === "error") {
      streamErrorHandled = true;
      const content = event.error.trim() || "模型连接或工具执行失败，等待处理。";
      const permissionMode = conversation.options.permissionMode ?? defaultPermissionMode;
      if (isApprovalRequired(content) && permissionMode === "approval") {
        if (confirmSessionFullAccess(conversation.title)) {
          const approvedState = withFullAccessConversation(stateForChat, conversation.id, messages);
          setState(approvedState);
          setRuns((current) => ({
            ...current,
            [conversation.id]: {
              runId: event.runId,
              phase: "running",
              message: "已获得本会话完全访问权限，正在自动继续执行。",
              todos: event.todos ?? current[conversation.id]?.todos ?? [],
              toolSteps: event.toolSteps ?? current[conversation.id]?.toolSteps ?? []
            }
          }));
          window.setTimeout(() => {
            void runConversationAgent({
              state: approvedState,
              setState,
              setRuns,
              conversationId: conversation.id,
              prompt: `继续执行刚才被权限拦截的任务：\n${prompt.trim()}`,
              onDone
            });
          }, 0);
          return;
        }
        const cancelled = "已取消本次权限请求，本轮执行已停止。";
        const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: cancelled, timestamp: new Date().toISOString() };
        setRuns((current) => ({
          ...current,
          [conversation.id]: {
            runId: event.runId,
            phase: "stopped",
            message: cancelled,
            todos: normalizeRunTodos(event.todos ?? current[conversation.id]?.todos, "stopped"),
            toolSteps: event.toolSteps ?? current[conversation.id]?.toolSteps ?? []
          }
        }));
        setState((current) => {
          const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks) } : current;
          const withPool = applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
          return { ...withPool, ...appendAssistantMessage(withPool, conversation.id, messages, assistant), tasks: pauseConversationTasks(withPool.tasks, conversation.id) };
        });
        void onDone?.(cancelled);
        return;
      }
      const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
      setRuns((current) => ({ ...current, [conversation.id]: { runId: event.runId, phase: "error", message: event.error, todos: normalizeRunTodos(event.todos ?? current[conversation.id]?.todos, "error"), toolSteps: event.toolSteps ?? current[conversation.id]?.toolSteps ?? [] } }));
      setState((current) => {
        const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks) } : current;
        const withPool = applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
        return { ...withPool, ...appendAssistantMessage(withPool, conversation.id, messages, assistant), tasks: pauseConversationTasks(withPool.tasks, conversation.id) };
      });
      void notifyAgentReply(conversation.title.startsWith("微信：") ? "微信会话执行异常" : "小亨执行异常", content);
      void trayNotifyAttention(conversation.title);
      void onDone?.(content);
    }
  });
  if (streamErrorHandled) return;
}

function Dashboard({ state, xiaoheng, activeTasks, donePercent, navigate }: { state: AppState; xiaoheng?: Agent; activeTasks: number; donePercent: number; navigate: (page: Page) => void }) {
  const enabledSkills = state.skills.filter((skill) => skill.enabled).length;
  const chartData = useMemo(() => {
    const labels = ["计划", "进行", "待验", "交付", "暂停", "停止"];
    const statuses: TaskStatus[] = ["todo", "doing", "review", "done", "paused", "stopped"];
    return statuses.map((status, index) => ({
      day: labels[index],
      done: state.tasks.filter((task) => task.status === "done" && statuses.indexOf(task.status) <= index).length,
      active: state.tasks.filter((task) => task.status === status).length + state.offices.length
    }));
  }, [state.tasks, state.offices.length]);
  return (
    <>
      <section className="hero-band">
        <div>
          <span className="eyebrow">今日工作概览</span>
          <h1>小亨已准备好组织团队执行任务</h1>
          <p>当前工作池包含 {state.agents.length} 位成员、{state.offices.length} 个项目组、{enabledSkills} 项启用技能和 {activeTasks} 个待推进任务。</p>
          <div className="hero-actions">
            <button className="primary" onClick={() => navigate("chat")}>
              <MessageCircle size={17} />
              进入对话
            </button>
            <button className="secondary" onClick={() => navigate("market")}>
              <PackagePlus size={17} />
              打开市场
            </button>
          </div>
        </div>
        <div className="hero-portrait"><img src="/xiaoheng-managers-cutout.png" alt="小亨经理人形象" /></div>
      </section>

      <div className="metric-grid">
        <Metric icon={CheckCircle2} label="任务完成率" value={`${donePercent}%`} detail="按当前任务池统计" />
        <Metric icon={BriefcaseBusiness} label="项目组" value={`${state.offices.length}`} detail="协作项目空间" />
        <Metric icon={Users} label="团队成员" value={`${state.agents.length}`} detail={`${state.agents.filter((agent) => agent.status === "working").length} 人工作中`} />
        <Metric icon={WandSparkles} label="启用技能" value={`${enabledSkills}`} detail="已进入对话上下文" />
      </div>

      <div className="dashboard-grid">
        <section className="panel">
          <div className="panel-title">
            <div>
              <span className="eyebrow">效率趋势</span>
              <h2>项目组活动</h2>
            </div>
          </div>
          <div className="chart">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={chartData}>
                <CartesianGrid stroke="#e5e7eb" vertical={false} />
                <XAxis dataKey="day" axisLine={false} tickLine={false} />
                <YAxis axisLine={false} tickLine={false} />
                <Tooltip />
                <Area type="monotone" dataKey="done" stroke="#2563eb" strokeWidth={3} fill="#dbeafe" />
                <Area type="monotone" dataKey="active" stroke="#0f766e" strokeWidth={2} fill="transparent" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </section>
        <section className="panel">
          <div className="panel-title">
            <div>
              <span className="eyebrow">待处理</span>
              <h2>优先任务</h2>
            </div>
            <button className="ghost" onClick={() => navigate("tasks")}>全部任务</button>
          </div>
          {state.tasks.filter((task) => task.status !== "done").slice(0, 5).map((task) => (
            <TaskRow key={task.id} task={task} agent={state.agents.find((agent) => agent.id === task.ownerId)} />
          ))}
        </section>
      </div>
    </>
  );
}

function Metric({ icon: Icon, label, value, detail }: { icon: LucideIcon; label: string; value: string; detail: string }) {
  return (
    <section className="metric">
      <Icon size={20} />
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </section>
  );
}

function ChatPage({
  state,
  setState,
  guidePrompt,
  clearGuidePrompt,
  runs,
  setRuns
}: {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  guidePrompt: string;
  clearGuidePrompt: () => void;
  runs: Record<string, RunSnapshot>;
  setRuns: React.Dispatch<React.SetStateAction<Record<string, RunSnapshot>>>;
}) {
  const [input, setInput] = useState("");
  const [pendingAttachments, setPendingAttachments] = useState<AttachmentItem[]>([]);
  const [skillDropdownOpen, setSkillDropdownOpen] = useState(false);
  const activeConversation = state.conversations.find((item) => item.id === state.activeConversationId) ?? state.conversations[0];
  const enabledSkills = state.skills.filter((skill) => skill.enabled);
  const selectedSkillIds = new Set(activeConversation?.options.skillIds ?? []);
  const activeMessages = activeConversation?.messages ?? state.messages;
  const activeSkillCount = selectedSkillIds.size || enabledSkills.length;
  const activeRun = activeConversation ? runs[activeConversation.id] : undefined;
  const sending = activeRun?.phase === "planning" || activeRun?.phase === "running";
  const selectableModels = selectableChatModels(state.settings);
  const activeAgent = state.agents.find((agent) => agent.id === activeConversation?.options.expertId) ?? state.agents.find((agent) => agent.role === "xiaoheng") ?? state.agents[0];

  useEffect(() => {
    if (!guidePrompt) return;
    setInput(guidePrompt);
    clearGuidePrompt();
  }, [guidePrompt, clearGuidePrompt]);

  const updateConversation = (id: string, patch: Partial<Conversation>) => {
    setState((current) => ({
      ...current,
      conversations: current.conversations.map((item) => item.id === id ? { ...item, ...patch, updatedAt: new Date().toISOString() } : item)
    }));
  };

  const newConversation = () => {
    const now = new Date().toISOString();
    const workspaceRoot = state.settings.workspaceRoot || defaultWorkspaceRoot;
    const item: Conversation = {
      id: `conversation-${crypto.randomUUID()}`,
      title: "新对话",
      createdAt: now,
      updatedAt: now,
      messages: [],
      options: {
        workspaceRoot,
        officeId: "",
        expertId: state.agents.find((agent) => agent.role === "xiaoheng")?.id,
        skillIds: enabledSkills.map((skill) => skill.id),
        model: state.settings.model,
        permissionMode: defaultPermissionMode
      }
    };
    setState((current) => ({
      ...current,
      settings: { ...current.settings, workspaceRoot: current.settings.workspaceRoot || workspaceRoot },
      conversations: [item, ...current.conversations],
      activeConversationId: item.id,
      messages: []
    }));
  };

  const deleteConversation = (id: string) => {
    if (id === HEARTBEAT_CONVERSATION_ID) {
      setState((current) => ({
        ...current,
        conversations: current.conversations.map((item) => item.id === id ? { ...item, messages: [], updatedAt: new Date().toISOString() } : item),
        messages: current.activeConversationId === id ? [] : current.messages
      }));
      return;
    }
    setState((current) => {
      const remaining = current.conversations.filter((item) => item.id !== id);
      const conversations = remaining.length ? remaining : structuredClone(defaultState.conversations);
      const activeConversationId = current.activeConversationId === id ? conversations[0].id : current.activeConversationId;
      const messages = conversations.find((item) => item.id === activeConversationId)?.messages ?? [];
      return { ...current, conversations, activeConversationId, messages };
    });
  };

  const uploadAttachments = async () => {
    const selected = await open({ directory: false, multiple: true, title: "选择要发送给小亨的文件" });
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    if (!paths.length) return;
    const registered: AttachmentItem[] = [];
    for (const path of paths) {
      registered.push(await registerAttachment(path));
    }
    setPendingAttachments((current) => [...current, ...registered]);
  };

  const send = async () => {
    if ((!input.trim() && !pendingAttachments.length) || sending || !activeConversation) return;
    const rawPrompt = input.trim();
    const basePrompt = rawPrompt || "请读取并处理我上传的附件。";
    const attachmentsToSend = pendingAttachments;
    const effectivePrompt = `${resolvePromptWithPendingObjective(basePrompt, activeConversation)}${attachmentSummary(attachmentsToSend)}`;
    const pendingObjective = nextPendingObjective(basePrompt, activeConversation);
    const runId = `run-${crypto.randomUUID()}`;
    const userMessage: ChatMessage = { id: crypto.randomUUID(), role: "user", content: basePrompt, timestamp: new Date().toISOString(), attachments: attachmentsToSend };
    const messages = [...activeMessages, userMessage];
    const messagesForModel = attachmentsToSend.length ? [...activeMessages, { ...userMessage, content: effectivePrompt }] : messages;
    const title = activeConversation.title === "新对话" ? basePrompt.slice(0, 24) : activeConversation.title;
    setState((current) => ({
      ...current,
      messages,
      conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, title, messages, options: { ...item.options, pendingObjective }, updatedAt: new Date().toISOString() } : item)
    }));
    setRuns((current) => ({
      ...current,
      [activeConversation.id]: {
        runId,
        phase: "running",
        message: "小亨 Gateway 正在处理请求。",
        todos: [],
        toolSteps: []
      }
    }));
    setInput("");
    setPendingAttachments([]);
    try {
      const stateForChat: AppState = {
        ...state,
        messages,
        activeConversationId: activeConversation.id,
        conversations: state.conversations.map((item) => item.id === activeConversation.id ? { ...item, title, messages, options: { ...item.options, pendingObjective } } : item),
        settings: resolveChatSettings({ ...state.settings, workspaceRoot: activeConversation.options.workspaceRoot }, activeConversation.options.model || state.agents.find((agent) => agent.id === activeConversation.options.expertId)?.defaultModel || state.settings.model)
      };
      let streamedContent = "";
      let streamErrorHandled = false;
      await completeChatStream(runId, messagesForModel, stateForChat.settings, stateForChat, (event) => {
        if (event.type === "started") {
          setRuns((current) => ({ ...current, [activeConversation.id]: { runId: event.runId, phase: "running", message: event.message ?? "小亨正在执行。", todos: current[activeConversation.id]?.todos ?? [], toolSteps: current[activeConversation.id]?.toolSteps ?? [] } }));
        }
        if (event.type === "todo") {
          setRuns((current) => ({ ...current, [activeConversation.id]: { ...(current[activeConversation.id] ?? { runId: event.runId, phase: "running", toolSteps: [] }), phase: "running", todos: event.todos } }));
        }
        if (event.type === "token") {
          streamedContent += event.content;
          setRuns((current) => ({ ...current, [activeConversation.id]: { ...(current[activeConversation.id] ?? { runId: event.runId, phase: "running", todos: [], toolSteps: [] }), phase: "running", message: "正在流式生成回复..." } }));
        }
        if (event.type === "content_reset") {
          streamedContent = event.content ?? "";
        }
        if (event.type === "assistant_note") {
          setRuns((current) => {
            const run = current[activeConversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
            return { ...current, [activeConversation.id]: { ...run, phase: "running", message: event.content } };
          });
        }
        if (event.type === "tool_start") {
          setRuns((current) => {
            const run = current[activeConversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
            return { ...current, [activeConversation.id]: { ...run, phase: "running", message: `正在调用工具：${event.step.name}`, toolSteps: upsertById(run.toolSteps, [event.step]) } };
          });
        }
        if (event.type === "tool_end") {
          setRuns((current) => {
            const run = current[activeConversation.id] ?? { runId: event.runId, phase: "running" as const, todos: [], toolSteps: [] };
            return { ...current, [activeConversation.id]: { ...run, phase: "running", message: `工具完成：${event.step.name}`, todos: event.todos?.length ? event.todos : run.todos, toolSteps: upsertById(run.toolSteps, [event.step]) } };
          });
          if (event.tasks?.length || event.workPoolUpdates) {
            setState((current) => {
              const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks!) } : current;
              return applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
            });
          }
        }
        if (event.type === "done") {
          const content = stripPseudoToolCallText(event.content) || stripPseudoToolCallText(streamedContent) || "本轮没有生成可显示的文本。";
          const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
          setRuns((current) => ({
            ...current,
            [activeConversation.id]: {
              runId: event.runId,
              phase: "done",
              message: (event.toolSteps.length || current[activeConversation.id]?.toolSteps.length) ? "本轮执行完成，工具结果和 todo 已保留在这里。" : "本轮回复完成。",
              todos: normalizeRunTodos(event.todos.length ? event.todos : current[activeConversation.id]?.todos, "done"),
              toolSteps: event.toolSteps.length ? event.toolSteps : (current[activeConversation.id]?.toolSteps ?? [])
            }
          }));
          setState((current) => {
            const nextMessages = [...(current.conversations.find((item) => item.id === activeConversation.id)?.messages ?? messages), assistant];
            return {
              ...current,
              ...applyWorkPoolUpdates({
                ...current,
                tasks: event.tasks.length ? upsertById(current.tasks, event.tasks.map((task) => task.status === "done" && !task.deliveredAt ? { ...task, deliveredAt: new Date().toISOString() } : task)) : current.tasks
              }, event.workPoolUpdates),
              messages: nextMessages,
              conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, messages: nextMessages, options: { ...item.options, pendingObjective: undefined }, updatedAt: new Date().toISOString() } : item)
            };
          });
          void notifyAgentReply("小亨已完成回复", content);
          void trayNotifyAttention(activeConversation.title);
        }
        if (event.type === "stopped") {
          const content = event.content?.trim() || "已停止本轮执行。";
          const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
          setRuns((current) => ({
            ...current,
            [activeConversation.id]: {
              runId: event.runId,
              phase: "stopped",
              message: "用户已停止本轮 Agent 执行。",
              todos: normalizeRunTodos(event.todos ?? current[activeConversation.id]?.todos, "stopped"),
              toolSteps: event.toolSteps ?? current[activeConversation.id]?.toolSteps ?? []
            }
          }));
          setState((current) => {
            const nextMessages = [...(current.conversations.find((item) => item.id === activeConversation.id)?.messages ?? messages), assistant];
            const alreadyStopped = current.conversations.find((item) => item.id === activeConversation.id)?.messages.some((message) => message.content === content && message.role === "assistant");
            return {
              ...current,
              tasks: current.tasks.map((task) => task.conversationId === activeConversation.id && ["todo", "doing", "review", "paused"].includes(task.status) ? { ...task, status: "stopped" } : task),
              messages: alreadyStopped ? current.messages : nextMessages,
              conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, messages: alreadyStopped ? item.messages : nextMessages, updatedAt: new Date().toISOString() } : item)
            };
          });
        }
        if (event.type === "error") {
          streamErrorHandled = true;
          const content = event.error.trim() || "模型连接或工具执行失败，等待处理。";
          const permissionMode = activeConversation.options.permissionMode ?? defaultPermissionMode;
          if (isApprovalRequired(content) && permissionMode === "approval") {
            if (confirmSessionFullAccess(activeConversation.title)) {
              const approvedState = withFullAccessConversation(stateForChat, activeConversation.id, messages);
              setState(approvedState);
              setRuns((current) => ({
                ...current,
                [activeConversation.id]: {
                  runId: event.runId,
                  phase: "running",
                  message: "已获得本会话完全访问权限，正在自动继续执行。",
                  todos: event.todos ?? current[activeConversation.id]?.todos ?? [],
                  toolSteps: event.toolSteps ?? current[activeConversation.id]?.toolSteps ?? []
                }
              }));
              window.setTimeout(() => {
                void runConversationAgent({
                  state: approvedState,
                  setState,
                  setRuns,
                  conversationId: activeConversation.id,
                  prompt: `继续执行刚才被权限拦截的任务：\n${effectivePrompt}`
                });
              }, 0);
              return;
            }
            const cancelled = "已取消本次权限请求，本轮执行已停止。";
            const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: cancelled, timestamp: new Date().toISOString() };
            setRuns((current) => ({
              ...current,
              [activeConversation.id]: {
                runId: event.runId,
                phase: "stopped",
                message: cancelled,
                todos: normalizeRunTodos(event.todos ?? current[activeConversation.id]?.todos, "stopped"),
                toolSteps: event.toolSteps ?? current[activeConversation.id]?.toolSteps ?? []
              }
            }));
            setState((current) => {
              const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks) } : current;
              const withPool = applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
              return { ...withPool, ...appendAssistantMessage(withPool, activeConversation.id, messages, assistant), tasks: pauseConversationTasks(withPool.tasks, activeConversation.id) };
            });
            return;
          }
          const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content, timestamp: new Date().toISOString() };
          setRuns((current) => ({
            ...current,
            [activeConversation.id]: {
              runId: event.runId,
              phase: "error",
              message: event.error,
              todos: normalizeRunTodos(event.todos ?? current[activeConversation.id]?.todos, "error"),
              toolSteps: event.toolSteps ?? current[activeConversation.id]?.toolSteps ?? []
            }
          }));
          setState((current) => {
            const withTasks = event.tasks?.length ? { ...current, tasks: upsertById(current.tasks, event.tasks) } : current;
            const withPool = applyWorkPoolUpdates(withTasks, event.workPoolUpdates);
            return { ...withPool, ...appendAssistantMessage(withPool, activeConversation.id, messages, assistant), tasks: pauseConversationTasks(withPool.tasks, activeConversation.id) };
          });
          void notifyAgentReply("小亨执行异常", content);
          void trayNotifyAttention(activeConversation.title);
        }
      });
      if (streamErrorHandled) return;
    } catch (error) {
      const errorMessage: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: `连接模型失败：${String(error)}`, timestamp: new Date().toISOString() };
      setState((current) => ({
        ...current,
        messages: [...current.messages, errorMessage],
        tasks: pauseConversationTasks(current.tasks, activeConversation.id),
        conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, messages: [...item.messages, errorMessage], updatedAt: new Date().toISOString() } : item)
      }));
      setRuns((current) => ({
        ...current,
        [activeConversation.id]: {
          runId,
          phase: "error",
          message: String(error),
          todos: normalizeRunTodos(current[activeConversation.id]?.todos, "error"),
          toolSteps: current[activeConversation.id]?.toolSteps ?? []
        }
      }));
    }
  };

  const stopCurrentRun = async () => {
    if (!activeConversation || !activeRun || !sending) return;
    await stopChatStream(activeRun.runId);
    const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: "已停止本轮执行。", timestamp: new Date().toISOString() };
    setRuns((current) => ({
      ...current,
      [activeConversation.id]: {
        ...activeRun,
        phase: "stopped",
        message: "用户已停止本轮 Agent 执行。",
        todos: normalizeRunTodos(activeRun.todos, "stopped")
      }
    }));
    setState((current) => {
      const currentMessages = current.conversations.find((item) => item.id === activeConversation.id)?.messages ?? current.messages;
      return {
        ...current,
        tasks: current.tasks.map((task) => task.conversationId === activeConversation.id && ["todo", "doing", "review", "paused"].includes(task.status) ? { ...task, status: "stopped" } : task),
        messages: [...currentMessages, assistant],
        conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, messages: [...item.messages, assistant], updatedAt: new Date().toISOString() } : item)
      };
    });
  };

  const browseWorkspace = async () => {
    if (!activeConversation) return;
    const selected = await open({ directory: true, multiple: false, title: "选择本次会话工作空间" });
    if (typeof selected !== "string") return;
    updateConversation(activeConversation.id, { options: { ...activeConversation.options, workspaceRoot: selected } });
    setState((current) => ({ ...current, settings: { ...current.settings, workspaceRoot: current.settings.workspaceRoot || selected } }));
  };

  return (
    <div className="chat-page">
      <PageHeader eyebrow="Agent 对话" title="与小亨协作" description="团队、技能、任务和后端工具会一起进入对话上下文。" />
      <div className="chat-workspace">
        <aside className="conversation-list panel">
          <button className="primary full" onClick={newConversation}><Plus size={16} />新建会话</button>
          {state.conversations.map((conversation) => (
            <button key={conversation.id} className={`conversation-item ${conversation.id === state.activeConversationId ? "active" : ""}`} onClick={() => setState((current) => ({ ...current, activeConversationId: conversation.id, messages: conversation.messages }))}>
              <MessageCircle size={15} />
              <span>{conversation.title}</span>
              <small>{conversation.messages.length} 条</small>
              <i onClick={(event) => { event.stopPropagation(); deleteConversation(conversation.id); }} title={conversation.id === HEARTBEAT_CONVERSATION_ID ? "清空心跳会话" : "删除会话"}>{conversation.id === HEARTBEAT_CONVERSATION_ID ? <Archive size={13} /> : <Trash2 size={13} />}</i>
            </button>
          ))}
        </aside>
        <div className="chat-layout">
          <section className="chat-window">
            <div className="chat-messages">
              {activeMessages.map((message) => (
                <div key={message.id} className={`message ${message.role}`}>
                  <div className="message-avatar">{message.role === "assistant" ? avatar(activeAgent, "sm") : <span className="avatar avatar-sm user-avatar">我</span>}</div>
                  <div className="bubble">
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{message.content}</ReactMarkdown>
                    <AttachmentCards attachments={message.attachments} />
                    {message.role === "assistant" && <GeneratedPathCards content={message.content} />}
                    <time>{new Date(message.timestamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</time>
                  </div>
                </div>
              ))}
              {sending && (
                <div className="message assistant">
                  <div className="message-avatar">{avatar(activeAgent, "sm")}</div>
                  <div className="bubble typing"><i /><i /><i /></div>
                </div>
              )}
            </div>
            <RunPanel run={activeRun} sending={sending} />
            <div className="composer">
              {!!pendingAttachments.length && (
                <div className="attachment-cards pending" style={{ display: "flex", flexWrap: "wrap", gap: 8, marginBottom: 8 }}>
                  {pendingAttachments.map((item) => (
                    <div className="attachment-card" key={item.id} title={item.cachePath} style={{ display: "flex", alignItems: "center", gap: 8, maxWidth: 360, padding: "8px 10px", border: "1px solid rgba(148, 163, 184, 0.35)", borderRadius: 8 }}>
                      <FileText size={16} />
                      <div>
                        <strong>{item.name}</strong>
                        <small style={{ display: "block", opacity: 0.72 }}>{formatByteSize(item.byteSize)}</small>
                      </div>
                      <button type="button" className="ghost icon" onClick={() => setPendingAttachments((current) => current.filter((attachment) => attachment.id !== item.id))} title="移除附件"><X size={14} /></button>
                    </div>
                  ))}
                </div>
              )}
              <textarea value={input} onChange={(event) => setInput(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void send(); } }} placeholder="例如：抓取腾讯网的头条，生成 Excel 放在桌面" />
              <div className="composer-options">
                <button className="secondary" type="button" onClick={() => void uploadAttachments()} disabled={sending} title="上传附件"><Paperclip size={15} />附件</button>
                <label className="workspace-option">工作空间<div className="inline-input"><input value={activeConversation?.options.workspaceRoot ?? ""} onChange={(event) => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, workspaceRoot: event.target.value } })} placeholder="留空使用系统默认工作区" /><button type="button" onClick={() => void browseWorkspace()}>浏览</button></div></label>
                <label>项目<select value={activeConversation?.options.officeId ?? ""} onChange={(event) => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, officeId: event.target.value } })}><option value="">不使用项目</option>{state.offices.map((office) => <option value={office.id} key={office.id}>{office.name}</option>)}</select></label>
                <label>专家<select value={activeConversation?.options.expertId ?? ""} onChange={(event) => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, expertId: event.target.value } })}><option value="">小亨自动选择</option>{state.agents.map((agent) => <option value={agent.id} key={agent.id}>{agent.name} · {agent.title}</option>)}</select></label>
                <label>模型<select value={activeConversation?.options.model ?? ""} onChange={(event) => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, model: event.target.value || undefined } })}><option value="">默认模型 · {state.settings.model}</option>{selectableModels.map((model) => <option value={model.id} key={model.id}>{modelIcon(model.id, state.settings.availableModels)} {model.name}</option>)}</select></label>
                <label>权限<select value={activeConversation?.options.permissionMode ?? defaultPermissionMode} onChange={(event) => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, permissionMode: event.target.value as SessionPermissionMode } })}>{(["restricted", "approval", "full"] as SessionPermissionMode[]).map((mode) => <option value={mode} key={mode}>{permissionModeLabels[mode]}</option>)}</select></label>
                <div className="skill-dropdown">
                  <button className="secondary" type="button" onClick={() => setSkillDropdownOpen((value) => !value)}><WandSparkles size={15} />技能 {activeSkillCount}</button>
                  {skillDropdownOpen && <div className="skill-menu">
                    <input placeholder="搜索技能..." onChange={(event) => {
                      const term = event.target.value.toLowerCase();
                      event.currentTarget.parentElement?.querySelectorAll("label").forEach((label) => {
                        label.toggleAttribute("hidden", !label.textContent?.toLowerCase().includes(term));
                      });
                    }} />
                    <button className="ghost" type="button" onClick={() => activeConversation && updateConversation(activeConversation.id, { options: { ...activeConversation.options, skillIds: enabledSkills.map((skill) => skill.id) } })}>全选技能</button>
                    {enabledSkills.map((skill) => (
                      <label key={skill.id}><input type="checkbox" checked={selectedSkillIds.size === 0 || selectedSkillIds.has(skill.id)} onChange={(event) => {
                        if (!activeConversation) return;
                        const next = new Set(selectedSkillIds.size ? selectedSkillIds : enabledSkills.map((item) => item.id));
                        if (event.target.checked) next.add(skill.id); else next.delete(skill.id);
                        updateConversation(activeConversation.id, { options: { ...activeConversation.options, skillIds: Array.from(next) } });
                      }} />{skill.name}</label>
                    ))}
                  </div>}
                </div>
                {sending && <button className="stop-button" onClick={() => void stopCurrentRun()} title="停止"><Square size={15} />停止</button>}
                <button className="send-button" onClick={() => void send()} disabled={(!input.trim() && !pendingAttachments.length) || sending} title="发送"><Send size={17} /></button>
              </div>
              <div className="quick-prompts">
                {["抓取百度新闻头条，生成 Excel 保存到桌面", "抓取腾讯网的头条，生成 Excel 放在桌面", "列出当前可用技能和人员分工"].map((text) => (
                  <button className="suggestion" key={text} onClick={() => setInput(text)}>{text}</button>
                ))}
              </div>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}

function RunPanel({ run, sending }: { run?: RunSnapshot; sending: boolean }) {
  if (!run && !sending) return null;
  const phase = run?.phase ?? "running";
  const title = phase === "planning" ? "正在规划" : phase === "running" ? "正在处理" : phase === "stopped" ? "已停止" : phase === "error" ? "执行异常" : "执行记录";
  const todos = run?.todos ?? [];
  return (
    <section className={`run-panel ${phase}`}>
      <header>
        <div>
          <span className="eyebrow">Agent Run</span>
          <strong>{title}</strong>
        </div>
        {(sending || phase === "planning" || phase === "running") && <LoaderCircle className="spin" size={16} />}
      </header>
      {run?.message && <p>{run.message}</p>}
      {!!todos.length && (
        <div className="run-todos">
          {todos.map((todo, index) => {
            const linkedStep = run?.toolSteps[index] ?? run?.toolSteps.find((step) => todo.content.toLowerCase().includes(step.name.toLowerCase()));
            return (
              <details className={`run-todo ${todo.status}`} key={todo.id}>
                <summary>
                  <span>{todo.status === "completed" ? <CheckCircle2 size={14} /> : todo.status === "cancelled" ? <X size={14} /> : todo.status === "in_progress" ? <LoaderCircle className="spin" size={14} /> : <Clock3 size={14} />}</span>
                  <b>{todo.content}</b>
                </summary>
                <pre>{linkedStep ? JSON.stringify({ tool: linkedStep.name, input: linkedStep.input, result: linkedStep.result, isError: linkedStep.isError }, null, 2) : "该步骤没有直接关联的工具调用；可能是模型规划、状态更新或人工等待步骤。"}</pre>
              </details>
            );
          })}
        </div>
      )}
      {!!run?.toolSteps.length && (
        <div className="run-tools">
          {run.toolSteps.map((step) => (
            <details className={`run-tool ${step.isError ? "error" : ""}`} key={step.id}>
              <summary><span>{step.name}</span><small>{step.isError ? "失败" : "完成"}</small></summary>
              <pre>{JSON.stringify({ input: step.input, result: step.result }, null, 2)}</pre>
            </details>
          ))}
        </div>
      )}
    </section>
  );
}

function OfficePage({ state, setState, openModal }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; openModal: (modal: ModalState) => void }) {
  const [expandedId, setExpandedId] = useState(state.offices[0]?.id ?? "");
  return (
    <>
      <PageHeader eyebrow="Projects" title="项目" description="呈现所有项目组、成员、目标交付内容、执行计划和当前进度。" action={<button className="primary" onClick={() => openModal({ type: "office" })}><Plus size={17} />新建项目组</button>} />
      <div className="office-grid">
        {state.offices.map((office) => {
          const members = [...office.managerIds, ...office.memberIds].map((id) => state.agents.find((agent) => agent.id === id)).filter(Boolean) as Agent[];
          const tasks = state.tasks.filter((task) => task.officeId === office.id);
          const done = tasks.filter((task) => task.status === "done").length;
          const progress = tasks.length ? Math.round((done / tasks.length) * 100) : office.progress;
          const expanded = expandedId === office.id;
          return (
            <section className="panel office-card" key={office.id}>
              <div className="panel-title">
                <div>
                  <span className="eyebrow">{tasks.length} 个任务</span>
                  <h2>{office.name}</h2>
                </div>
                <button className="icon-btn flat" onClick={() => openModal({ type: "office", item: office })} title="编辑项目组"><Settings size={16} /></button>
              </div>
              <p>{office.description}</p>
              <div className="progress"><i style={{ width: `${progress}%` }} /></div>
              <div className="office-footer">
                <div className="stacked-avatars">{members.slice(0, 5).map((agent) => <span key={agent.id}>{avatar(agent, "sm")}</span>)}</div>
                <strong>{progress}%</strong>
              </div>
              {expanded && (
                <div className="project-detail">
                  <ProjectList title="目标" items={office.goals} fallback="小亨会在创建 todo 后动态补充项目目标。" />
                  <ProjectList title="交付物" items={office.deliverables} fallback="暂无明确交付物。" />
                  <ProjectList title="计划" items={office.plan} fallback="暂无执行计划。" />
                  <div className="project-status-grid">
                    {(["todo", "doing", "review", "done", "paused", "stopped"] as TaskStatus[]).map((status) => (
                      <span key={status}><b>{tasks.filter((task) => task.status === status).length}</b>{statusLabel(status)}</span>
                    ))}
                  </div>
                </div>
              )}
              <div className="card-actions">
                <button onClick={() => setExpandedId(expanded ? "" : office.id)}>{expanded ? "收起" : "展开"}</button>
                <button onClick={() => openModal({ type: "task", item: undefined })}>新建任务</button>
                <button onClick={() => setState((current) => ({ ...current, offices: current.offices.filter((item) => item.id !== office.id) }))}>删除</button>
              </div>
            </section>
          );
        })}
      </div>
    </>
  );
}

function ProjectList({ title, items, fallback }: { title: string; items?: string[]; fallback: string }) {
  const list = items?.filter(Boolean) ?? [];
  return (
    <div className="project-list">
      <strong>{title}</strong>
      {list.length ? list.map((item) => <span key={item}>{item}</span>) : <small>{fallback}</small>}
    </div>
  );
}

function TeamPage({ state, setState, openModal }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; openModal: (modal: ModalState) => void }) {
  const agentTasks = (agentId: string) => state.tasks.filter((task) => task.ownerId === agentId && !task.archived);
  const agentStatus = (agentId: string): Agent["status"] => {
    const tasks = agentTasks(agentId);
    if (tasks.some((task) => task.status === "doing" || task.status === "review")) return "working";
    if (tasks.some((task) => task.status === "todo" || task.status === "paused")) return "online";
    return "idle";
  };
  return (
    <>
      <PageHeader eyebrow="People" title="人员" description="列出所有预设和市场安装的专家成员；小亨协调，经理拆解验收，职员执行交付。" action={<button className="primary" onClick={() => openModal({ type: "agent" })}><Plus size={17} />新增成员</button>} />
      <div className="agent-grid">
        {state.agents.map((agent) => (
          <AgentCard
            key={agent.id}
            agent={agent}
            status={agentStatus(agent.id)}
            tasks={agentTasks(agent.id)}
            settings={state.settings}
            onEdit={() => openModal({ type: "agent", item: agent })}
            onDelete={agent.role === "xiaoheng" ? undefined : () => setState((current) => ({
              ...current,
              agents: current.agents.filter((item) => item.id !== agent.id),
              offices: current.offices.map((office) => ({ ...office, managerIds: office.managerIds.filter((id) => id !== agent.id), memberIds: office.memberIds.filter((id) => id !== agent.id) })),
              tasks: current.tasks.map((task) => task.ownerId === agent.id ? { ...task, ownerId: "xiaoheng" } : task)
            }))}
          />
        ))}
      </div>
    </>
  );
}

function AgentCard({ agent, status, tasks, settings, onEdit, onDelete }: { agent: Agent; status: Agent["status"]; tasks: WorkTask[]; settings: AppState["settings"]; onEdit: () => void; onDelete?: () => void }) {
  const modelId = agent.defaultModel || settings.model;
  const modelName = agent.defaultModel ? modelId : `跟随全局 · ${settings.model}`;
  const currentTasks = tasks.filter((task) => task.status !== "done" && task.status !== "stopped");
  const doingCount = tasks.filter((task) => task.status === "doing" || task.status === "review").length;
  const todoCount = tasks.filter((task) => task.status === "todo").length;
  const pausedCount = tasks.filter((task) => task.status === "paused").length;
  return (
    <section className="agent-card">
      {avatar(agent, "lg")}
      <span className={`agent-status ${status}`} />
      <h3>{agent.name}</h3>
      <b>{agent.title}</b>
      <p>{agent.specialty}</p>
      <div className="agent-model"><span>{modelIcon(modelId, settings.availableModels)}</span><div><strong>默认模型</strong><small>{modelName}</small></div></div>
      <div className="agent-workload" aria-label={`${agent.name} 当前工作状态`}>
        <span><b>{doingCount}</b>进行/验收</span>
        <span><b>{todoCount}</b>计划中</span>
        <span><b>{pausedCount}</b>已暂停</span>
      </div>
      <div className="agent-current-tasks">
        {currentTasks.length ? currentTasks.slice(0, 3).map((task) => (
          <small key={task.id}><b>{statusLabel(task.status)}</b>{task.title}</small>
        )) : <small className="empty">当前没有分配任务</small>}
      </div>
      <footer>
        <span>{roleLabel(agent.role)}</span>
        <span>{status === "working" ? "工作中" : status === "online" ? "在线" : "空闲"}</span>
      </footer>
      <div className="card-actions">
        <button onClick={onEdit}>编辑</button>
        {onDelete && <button onClick={onDelete}>删除</button>}
      </div>
    </section>
  );
}

function TasksPage({ state, setState, openModal, onRetry }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; openModal: (modal: ModalState) => void; onRetry: (task: WorkTask) => void }) {
  const [selectedId, setSelectedId] = useState(state.tasks[0]?.id ?? "");
  const [historyOpen, setHistoryOpen] = useState(false);
  const move = (id: string, status: TaskStatus, description?: string) => setState((current) => ({
    ...current,
    tasks: current.tasks.map((task) => task.id === id ? { ...task, status, deliveredAt: status === "done" ? task.deliveredAt ?? new Date().toISOString() : task.deliveredAt, description: description ? `${task.description}\n${description}` : task.description } : task)
  }));
  const activeTasks = state.tasks.filter((task) => !task.archived);
  const deliveredTasks = activeTasks.filter((task) => task.status === "done").sort((a, b) => Date.parse(b.deliveredAt ?? b.dueDate) - Date.parse(a.deliveredAt ?? a.dueDate));
  const latestDelivered = deliveredTasks[0];
  const archivedDelivered = deliveredTasks.slice(1);
  const selected = activeTasks.find((task) => task.id === selectedId) ?? activeTasks[0];
  const selectedOwner = state.agents.find((agent) => agent.id === selected?.ownerId);
  const upstream = selected?.upstreamIds?.map((id) => state.agents.find((agent) => agent.id === id)).filter(Boolean) as Agent[] || state.agents.filter((agent) => agent.role === "manager").slice(0, 1);
  const downstream = selected?.downstreamIds?.map((id) => state.agents.find((agent) => agent.id === id)).filter(Boolean) as Agent[] || state.agents.filter((agent) => agent.role === "har").slice(0, 2);
  const archiveTask = async (task: WorkTask) => {
    const archived = { ...task, archived: true, archivedAt: new Date().toISOString() };
    setState((current) => ({ ...current, tasks: current.tasks.map((item) => item.id === task.id ? archived : item) }));
    await archiveTaskTree(archived);
  };
  const groups: { key: TaskStatus; title: string }[] = [
    { key: "todo", title: "计划中" },
    { key: "doing", title: "进行中" },
    { key: "review", title: "待验收" },
    { key: "done", title: "已交付" },
    { key: "paused", title: "已暂停" },
    { key: "stopped", title: "已停止" }
  ];
  return (
    <>
      <PageHeader eyebrow="Tasks" title="任务中心" description="实时列出计划、进行中、待验收、已交付、暂停和停止的工作内容及对应人员。" action={<button className="primary" onClick={() => openModal({ type: "task" })}><Plus size={17} />新建任务</button>} />
      <div className="task-board-layout">
        <section className="task-columns">
          {groups.map((group) => (
            <div className="task-column" key={group.key}>
              <header><strong>{group.title}</strong><span>{group.key === "done" ? deliveredTasks.length : activeTasks.filter((task) => task.status === group.key).length}</span></header>
              {(group.key === "done" ? (latestDelivered ? [latestDelivered] : []) : activeTasks.filter((task) => task.status === group.key)).map((task) => {
                const agent = state.agents.find((item) => item.id === task.ownerId);
                return <article className={`task-mini ${selected?.id === task.id ? "active" : ""}`} key={task.id} onClick={() => setSelectedId(task.id)}>
                  <h3>{task.title}</h3>
                  <p>{task.description}</p>
                  <footer><span className="owner">{avatar(agent, "sm")}{agent?.name}</span><span className={`priority small ${task.priority}`}>{priorityLabel(task.priority)}</span></footer>
                  <div className="row-actions">
                    {(task.status === "paused" || task.status === "stopped") && <button onClick={(event) => { event.stopPropagation(); onRetry(task); }}>重试</button>}
                    {task.status === "done" ? <button onClick={(event) => { event.stopPropagation(); void archiveTask(task); }}><Archive size={13} />归档</button> : <><button onClick={(event) => { event.stopPropagation(); move(task.id, "paused", "用户手动标记暂停：等待授权、补充信息或错误处理。"); }}>暂停</button><button onClick={(event) => { event.stopPropagation(); move(task.id, "stopped", "用户手动停止该任务。"); }}>停止</button></>}
                    <button onClick={(event) => { event.stopPropagation(); setState((current) => ({ ...current, tasks: current.tasks.filter((item) => item.id !== task.id) })); }}>删除</button>
                  </div>
                </article>;
              })}
              {group.key === "done" && archivedDelivered.length > 0 && <button className="secondary full history-button" onClick={() => setHistoryOpen(true)}>查看以前的任务</button>}
            </div>
          ))}
        </section>
        {selected && <aside className="panel task-detail">
          <div className="panel-title"><div><span className="eyebrow">任务详情</span><h2>{selected.title}</h2></div><button className="secondary" onClick={() => openModal({ type: "task", item: selected })}>编辑</button></div>
          <p>{selected.description}</p>
          <div className="task-meta"><span>负责人：{selectedOwner?.name ?? "未指定"}</span><span>状态：{statusLabel(selected.status)}</span><span>截止：{selected.dueDate}</span></div>
          <div className="flow-line">
            <div><strong>上游</strong>{upstream.map((agent) => <span key={agent.id}>{avatar(agent, "sm")}{agent.name}</span>)}</div>
            <i />
            <div><strong>执行</strong>{selectedOwner && <span>{avatar(selectedOwner, "sm")}{selectedOwner.name}</span>}</div>
            <i />
            <div><strong>下游</strong>{downstream.map((agent) => <span key={agent.id}>{avatar(agent, "sm")}{agent.name}</span>)}</div>
          </div>
          <div className="task-actions">{(selected.status === "paused" || selected.status === "stopped") && <button className="primary" onClick={() => onRetry(selected)}>重试</button>}{selected.status === "done" ? <button className="primary" onClick={() => void archiveTask(selected)}><Archive size={15} />归档</button> : <><button className="secondary" onClick={() => move(selected.id, "paused", "用户手动标记暂停：等待授权、补充信息或错误处理。")}>暂停</button><button className="secondary" onClick={() => move(selected.id, "stopped", "用户手动停止该任务。")}>停止</button><button className="primary" onClick={() => move(selected.id, "done")}>标记交付</button></>}</div>
        </aside>}
      </div>
      <AnimatePresence>
        {historyOpen && <TaskHistoryModal tasks={archivedDelivered} agents={state.agents} onSelect={(task) => { setSelectedId(task.id); setHistoryOpen(false); }} onClose={() => setHistoryOpen(false)} />}
      </AnimatePresence>
    </>
  );
}

function TaskHistoryModal({ tasks, agents, onSelect, onClose }: { tasks: WorkTask[]; agents: Agent[]; onSelect: (task: WorkTask) => void; onClose: () => void }) {
  const groups = tasks.reduce<Record<string, WorkTask[]>>((acc, task) => {
    const day = (task.deliveredAt ?? task.dueDate ?? new Date().toISOString()).slice(0, 10);
    acc[day] = [...(acc[day] ?? []), task];
    return acc;
  }, {});
  return (
    <motion.div className="modal-backdrop" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={onClose}>
      <motion.div className="modal task-history-modal" initial={{ scale: 0.96, y: 18 }} animate={{ scale: 1, y: 0 }} onMouseDown={(event) => event.stopPropagation()}>
        <header><div><span className="eyebrow">Delivered</span><h2>以前的已交付任务</h2></div><button className="icon-btn flat" onClick={onClose}><X size={18} /></button></header>
        {Object.entries(groups).map(([day, items]) => (
          <section className="history-day" key={day}>
            <h3>{day}</h3>
            {items.map((task) => {
              const agent = agents.find((item) => item.id === task.ownerId);
              return <button className="history-task" key={task.id} onClick={() => onSelect(task)}><span>{task.title}</span><small>{agent?.name ?? "未指定"} · {task.description.slice(0, 80)}</small></button>;
            })}
          </section>
        ))}
      </motion.div>
    </motion.div>
  );
}

function SkillsPage({ state, setState, openModal, onInstall }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; openModal: (modal: ModalState) => void; onInstall: (payload: InstallPayload) => void }) {
  const installDefaults = async () => onInstall(await installDefaultSkills());
  return (
    <>
      <PageHeader eyebrow="Skills" title="技能中心" description="启用后的技能会进入小亨对话上下文，并参与工具调用决策。" action={<div className="header-actions"><button className="secondary" onClick={() => void installDefaults()}><Download size={17} />安装默认技能</button><button className="primary" onClick={() => openModal({ type: "skill" })}><Plus size={17} />新增技能</button></div>} />
      <div className="skill-grid">
        {state.skills.map((skill) => (
          <section className="skill-card" key={skill.id}>
            <div className="skill-top">
              <WandSparkles size={20} />
              <label className="switch"><input type="checkbox" checked={skill.enabled} onChange={() => setState((current) => ({ ...current, skills: current.skills.map((item) => item.id === skill.id ? { ...item, enabled: !item.enabled } : item) }))} /><i /></label>
            </div>
            <span className="chip">{skill.category}</span>
            <h3>{skill.name}</h3>
            <p>{skill.description}</p>
            <small>{skill.tools?.join(" · ") || skill.source || "custom"}</small>
            <div className="card-actions">
              <button onClick={() => openModal({ type: "skill", item: skill })}>编辑</button>
              <button onClick={() => setState((current) => ({ ...current, skills: current.skills.filter((item) => item.id !== skill.id) }))}>删除</button>
            </div>
          </section>
        ))}
      </div>
    </>
  );
}

function MarketplacePage({
  state,
  setState,
  openModal,
  onInstall,
  installStatus,
  setInstallStatus
}: {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  openModal: (modal: ModalState) => void;
  onInstall: (payload: InstallPayload) => void;
  installStatus: Record<string, SkillInstallStatus>;
  setInstallStatus: React.Dispatch<React.SetStateAction<Record<string, SkillInstallStatus>>>;
}) {
  const [market, setMarket] = useState<MarketplaceAggregate | null>(null);
  const [tab, setTab] = useState<"experts" | "teams" | "skills">("experts");
  const [view, setView] = useState<"market" | "installed">("market");
  const [skillSource, setSkillSource] = useState<"harmony" | "clawhub" | "skillhub">("harmony");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState("");
  const [remoteSkills, setRemoteSkills] = useState<MarketItem[]>([]);
  const [installedSkills, setInstalledSkills] = useState<SkillItem[]>([]);
  const [locator, setLocator] = useState("");

  useEffect(() => {
    fetchMarketplace().then(setMarket).catch((error) => console.error(error));
    listInstalledMarketSkills().then((result) => {
      setInstalledSkills(result.skills);
      if (result.skills.length) setState((current) => mergeInstalledSkills(current, result.skills));
    }).catch((error) => console.error(error));
  }, [setState]);

  useEffect(() => {
    if (tab !== "skills" || view !== "market") return;
    searchSkillMarket(skillSource, query, 24, 0).then((result) => setRemoteSkills(result.items)).catch((error) => console.error(error));
  }, [tab, view, skillSource]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<SkillInstallProgressEvent>("xiaoheng://skill-install-progress", (event) => {
      if (disposed) return;
      const payload = event.payload;
      if (!payload.slug) return;
      const source = payload.source ?? "harmony";
      const key = `${source}:${payload.slug}`;
      setInstallStatus((current) => {
        const previous = current[key];
        const stage = payload.stage ?? previous?.stage;
        return {
          ...current,
          [key]: {
            source,
            slug: payload.slug!,
            percent: Math.max(payload.percent ?? 0, previous?.percent ?? 0),
            stage,
            busy: stage !== "done" && stage !== "error",
            error: payload.error
          }
        };
      });
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [setInstallStatus]);

  const items = useMemo(() => {
    if (tab !== "skills") return [];
    const list = remoteSkills;
    const q = query.trim().toLowerCase();
    return q ? list.filter((item) => `${item.name} ${item.description} ${item.tags?.join(" ")}`.toLowerCase().includes(q)) : list;
  }, [market, query, tab, skillSource, remoteSkills]);

  const installedItems = useMemo(() => {
    if (tab === "experts") return state.agents.filter((agent) => agent.sourceId).map((agent) => ({
      id: agent.id,
      name: agent.name,
      description: agent.specialty,
      source: "installed",
      slug: agent.sourceId
    } as MarketItem));
    if (tab === "teams") return state.offices.filter((office) => office.sourceId).map((office) => ({
      id: office.id,
      name: office.name,
      description: office.description,
      source: "installed",
      slug: office.sourceId
    } as MarketItem));
    return installedSkills.map((skill) => ({
      id: skill.id,
      name: skill.name,
      description: skill.description,
      source: skill.source,
      slug: skill.sourceId?.split(":").slice(1).join(":") || skill.id.replace(/^skill-[^-]+-/, ""),
      installPath: skill.installPath,
      version: skill.version
    } as MarketItem));
  }, [tab, state.agents, state.offices, installedSkills]);

  const marketKey = (item: MarketItem) => {
    const source = item.source ?? skillSource;
    const slug = item.slug ?? item.id.split("/").pop()?.split("@")[0] ?? item.id;
    return `${source}:${slug}`;
  };

  const marketSlug = (item: MarketItem) => item.slug ?? item.id.split("/").pop()?.split("@")[0] ?? item.id;

  const isMarketSkillInstalled = (item: MarketItem) => {
    const source = item.source ?? skillSource;
    const slug = marketSlug(item);
    const sourceId = `${source}:${slug}`;
    return [...state.skills, ...installedSkills].some((skill) =>
      skill.sourceId === sourceId ||
      skill.id === `skill-${source}-${slug}` ||
      (skill.source === source && skill.sourceId?.endsWith(`:${slug}`))
    );
  };

  const install = async (item: MarketItem) => {
    if (tab === "skills" && isMarketSkillInstalled(item)) return;
    setBusy(item.id);
    const source = item.source ?? skillSource;
    const slug = marketSlug(item);
    const statusKey = marketKey(item);
    setInstallStatus((current) => ({ ...current, [statusKey]: { source, slug, percent: 0, stage: "starting", busy: true } }));
    try {
      const payload = tab === "experts"
        ? await installMarketExpert(item)
        : tab === "teams"
          ? await installMarketTeam(item)
          : item.source === "harmony" || item.source === "clawhub" || item.source === "skillhub"
            ? await installRegistrySkill(item)
            : await installMarketSkill(item);
      onInstall(payload);
      if (payload.skills?.length) {
        setInstalledSkills((current) => upsertById(current, payload.skills!));
        setState((current) => mergeInstalledSkills(current, payload.skills!));
      }
      setInstallStatus((current) => ({ ...current, [statusKey]: { source, slug, percent: 100, stage: "done", busy: false } }));
    } catch (error) {
      setInstallStatus((current) => ({ ...current, [statusKey]: { source, slug, percent: current[statusKey]?.percent ?? 0, stage: "error", busy: false, error: String(error) } }));
      throw error;
    } finally {
      setBusy("");
    }
  };

  const runSearch = async () => {
    if (tab === "skills") {
      setBusy(`search-${skillSource}`);
      try {
        const result = await searchSkillMarket(skillSource, query, 24, 0);
        setRemoteSkills(result.items);
      } finally {
        setBusy("");
      }
    }
  };

  const installLocator = async () => {
    if (!locator.trim()) return;
    setBusy("locator");
    try {
      const payload = await installSkillFromLocator(locator.trim());
      onInstall(payload);
      if (payload.skills?.length) {
        setInstalledSkills((current) => upsertById(current, payload.skills!));
        setState((current) => mergeInstalledSkills(current, payload.skills!));
      }
      setLocator("");
    } finally {
      setBusy("");
    }
  };

  const uninstall = async (item: MarketItem) => {
    if (tab !== "skills") return;
    const skill = state.skills.find((entry) => entry.id === item.id) ?? installedSkills.find((entry) => entry.id === item.id);
    const source = item.source ?? skill?.source ?? "local";
    const slug = item.slug ?? skill?.sourceId?.split(":").slice(1).join(":") ?? item.id.replace(/^skill-[^-]+-/, "");
    setBusy(`uninstall-${item.id}`);
    try {
      await uninstallMarketSkill(source, slug);
      setState((current) => ({ ...current, skills: current.skills.filter((entry) => entry.id !== item.id) }));
      setInstalledSkills((current) => current.filter((entry) => entry.id !== item.id));
    } finally {
      setBusy("");
    }
  };

  const visibleItems = view === "installed" ? installedItems : items;

  return (
    <>
      <PageHeader eyebrow="Marketplace" title="市场" description="当前版本专家和团队仅支持本地已安装管理；技能支持 Harmony、ClawHub、SkillHub 和本地/URL 安装。" />
      <section className="market-toolbar">
        <div className="tabs">
          <button className={tab === "experts" ? "active" : ""} onClick={() => setTab("experts")}>专家</button>
          <button className={tab === "teams" ? "active" : ""} onClick={() => setTab("teams")}>团队</button>
          <button className={tab === "skills" ? "active" : ""} onClick={() => setTab("skills")}>技能</button>
        </div>
        <div className="tabs small-tabs">
          <button className={view === "market" ? "active" : ""} onClick={() => setView("market")}>市场</button>
          <button className={view === "installed" ? "active" : ""} onClick={() => setView("installed")}>已安装</button>
        </div>
      </section>
      {tab === "skills" && view === "market" && <div className="source-row source-buttons"><button className={skillSource === "harmony" ? "active" : ""} onClick={() => setSkillSource("harmony")}>Harmony</button><button className={skillSource === "clawhub" ? "active" : ""} onClick={() => setSkillSource("clawhub")}>ClawHub</button><button className={skillSource === "skillhub" ? "active" : ""} onClick={() => setSkillSource("skillhub")}>SkillHub</button></div>}
      {tab === "skills" && view === "installed" && <section className="install-box panel"><strong>安装技能</strong><div className="inline-input"><input value={locator} onChange={(event) => setLocator(event.target.value)} placeholder="粘贴 SKILL.md URL、GitHub raw URL 或本地 SKILL.md/技能目录路径..." /><button onClick={() => void installLocator()} disabled={busy === "locator"}>{busy === "locator" ? "安装中" : "安装"}</button></div><small>支持本地路径、直接 SKILL.md URL。zip 包解析会在后续版本补齐。</small></section>}
      {view === "market" && tab === "skills" && <section className="market-search"><div className="searchbox compact"><Search size={16} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={skillSource === "harmony" ? "搜索 Harmony 内部技能" : "搜索真实技能市场，例如 excel、github、browser"} onKeyDown={(event) => { if (event.key === "Enter") void runSearch(); }} /></div><button className="primary" onClick={() => void runSearch()} disabled={busy.startsWith("search-")}>{busy.startsWith("search-") ? <LoaderCircle className="spin" size={16} /> : <Search size={16} />}搜索</button></section>}
      {view === "market" && tab !== "skills" && <section className="empty-market panel"><h3>共享社区功能后续开放</h3><p>当前版本的专家和团队不从公共市场下载，仅支持在“已安装”页面创建和管理。</p></section>}
      {view === "market" && tab === "skills" && skillSource === "harmony" && !remoteSkills.length && <section className="empty-market panel"><h3>Harmony 技能市场</h3><p>当前服务器暂无可安装技能。上传 .skill.zip 后会自动出现在这里。</p></section>}
      {view === "installed" && tab !== "skills" && <section className="market-local-actions"><button className="primary" onClick={() => openModal({ type: tab === "experts" ? "agent" : "office" })}><Plus size={16} />{tab === "experts" ? "创建新专家" : "创建新团队"}</button></section>}
      <div className="market-grid">
        {visibleItems.map((item) => {
          const status = installStatus[marketKey(item)];
          const installed = view === "market" && tab === "skills" && isMarketSkillInstalled(item);
          const installing = Boolean(status?.busy);
          return (
            <section className="market-card" key={item.id}>
              <div className="market-title">
                <PackagePlus size={20} />
                <span>{installed ? "已安装" : item.featured ? "推荐" : item.source || "openpisci"}</span>
              </div>
              <h3>{item.name}</h3>
              <p>{item.description}</p>
              <small>{item.installPath || item.id}{item.downloads ? ` · ${item.downloads} installs` : ""}{item.stars ? ` · ★ ${item.stars}` : ""}</small>
              {view === "market" && (installing || status?.error) && (
                <div className={`install-progress ${status?.error ? "error" : ""}`}>
                  <span><Download size={13} />{status?.error ? "安装失败" : status?.stage === "unpacking" ? "正在解包" : status?.stage === "downloaded" ? "下载完成，准备安装" : "下载并安装中"}</span>
                  <b>{status?.percent ?? 0}%</b>
                  <i><em style={{ width: `${status?.percent ?? 0}%` }} /></i>
                  {status?.error && <small>{status.error}</small>}
                </div>
              )}
              {view === "market" ? (
                installed ? (
                  <button className="secondary full" disabled><CheckCircle2 size={16} />已安装</button>
                ) : (
                  <button className="primary full" onClick={() => void install(item)} disabled={installing || busy === item.id}>
                    {installing || busy === item.id ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
                    {installing ? "安装中" : "安装"}
                  </button>
                )
              ) : tab === "skills" ? <div className="installed-actions"><label className="switch"><input type="checkbox" checked={state.skills.find((skill) => skill.id === item.id)?.enabled ?? true} onChange={() => setState((current) => ({ ...current, skills: current.skills.map((skill) => skill.id === item.id ? { ...skill, enabled: !skill.enabled } : skill) }))} /><i /></label><button onClick={() => void uninstall(item)} disabled={busy === `uninstall-${item.id}`}>{busy === `uninstall-${item.id}` ? <LoaderCircle className="spin" size={15} /> : <Trash2 size={15} />}删除</button></div> : <span className="installed-line"><CheckCircle2 size={16} /> 已安装</span>}
            </section>
          );
        })}
      </div>
    </>
  );
}

function ImPage({
  state,
  setState,
  runs,
  setRuns
}: {
  state: AppState;
  setState: React.Dispatch<React.SetStateAction<AppState>>;
  runs: Record<string, RunSnapshot>;
  setRuns: React.Dispatch<React.SetStateAction<Record<string, RunSnapshot>>>;
}) {
  const [channels, setChannels] = useState<ImChannelStatus[]>([]);
  const [tab, setTab] = useState<"channels" | "sessions">("channels");
  const [busy, setBusy] = useState("");
  const [drafts, setDrafts] = useState<Record<string, Record<string, string>>>({});
  const [message, setMessage] = useState("");
  const [replyInput, setReplyInput] = useState("");
  const [weixinLogin, setWeixinLogin] = useState<{ sessionKey?: string; qrcodeUrl?: string; status?: string; message?: string }>({});
  const activeSession = state.imSessions.find((item) => item.id === state.activeImSessionId) ?? state.imSessions[0];
  const activeConversation = activeSession ? state.conversations.find((item) => item.id === activeSession.conversationId) : undefined;
  const activeRun = activeConversation ? runs[activeConversation.id] : undefined;
  const sessionSending = activeRun?.phase === "planning" || activeRun?.phase === "running";
  const refresh = () => listImChannels().then(setChannels).catch((error) => console.error(error));
  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    listen<{ status?: string; sessionKey?: string; qrcodeUrl?: string; message?: string; error?: string }>("xiaoheng://weixin-login", (event) => {
      if (disposed) return;
      const payload = event.payload;
      if (payload.error) {
        setMessage(payload.error);
        setBusy("");
        return;
      }
      setWeixinLogin({
        sessionKey: payload.sessionKey,
        qrcodeUrl: payload.qrcodeUrl,
        status: payload.status,
        message: payload.message
      });
      if (payload.status === "connected") void startWeixinMonitor();
      setMessage(payload.message ?? "");
      setBusy("");
    }).then((fn) => {
      unlisten = fn;
    }).catch((error) => console.error(error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!weixinLogin.sessionKey || weixinLogin.status === "connected") return;
    const timer = window.setInterval(() => {
      pollWeixinLogin(weixinLogin.sessionKey!)
        .then((result) => {
          setWeixinLogin((current) => ({ ...current, status: result.status, message: result.message }));
          if (result.message) setMessage(result.message);
          if (result.status === "connected") {
            window.clearInterval(timer);
            void startWeixinMonitor();
            void refresh();
          }
          if (result.status === "expired" || result.status === "error") {
            window.clearInterval(timer);
          }
        })
        .catch((error) => {
          setMessage(String(error));
          window.clearInterval(timer);
        });
    }, 2500);
    return () => window.clearInterval(timer);
  }, [weixinLogin.sessionKey, weixinLogin.status]);

  const toggle = async (channel: ImChannelStatus) => {
    setBusy(channel.id);
    try {
      if (channel.status === "connected") await disconnectImChannel(channel.id);
      else if (channel.connectionMode === "xiaoheng-weixin-native") {
        setWeixinLogin({});
        const result = await installWeixinChannel(true);
        setMessage(result.message ?? "正在生成微信扫码二维码。");
        if (result.status === "connected") void startWeixinMonitor();
        if (result.status === "qrcode" || result.status === "connected") {
          setWeixinLogin({
            sessionKey: result.sessionKey,
            qrcodeUrl: result.qrcodeUrl,
            status: result.status,
            message: result.message
          });
        }
      }
      else await connectImChannel(channel.id);
      refresh();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy("");
    }
  };

  const updateDraft = (channel: ImChannelStatus, key: string, value: string) => {
    setDrafts((current) => ({
      ...current,
      [channel.id]: {
        ...(Object.fromEntries(Object.entries(channel.config ?? {}).map(([k, v]) => [k, String(v)]))),
        ...(current[channel.id] ?? {}),
        [key]: value
      }
    }));
  };

  const saveConfig = async (channel: ImChannelStatus) => {
    setBusy(`save-${channel.id}`);
    try {
      const config = { ...(channel.config ?? {}), ...(drafts[channel.id] ?? {}) };
      await saveImChannelConfig(channel.id, config);
      await refresh();
    } finally {
      setBusy("");
    }
  };

  const testChannel = async (channel: ImChannelStatus) => {
    setBusy(`test-${channel.id}`);
    try {
      await testImChannel(channel.id, "小亨 WorkBuddy IM 测试消息");
      if (channel.connectionMode === "xiaoheng-weixin-native") await startWeixinMonitor();
      await refresh();
    } finally {
      setBusy("");
    }
  };

  const selectSession = (sessionId: string) => {
    setState((current) => ({
      ...current,
      activeImSessionId: sessionId,
      imSessions: current.imSessions.map((item) => item.id === sessionId ? { ...item, unread: 0 } : item)
    }));
  };

  const sendToActiveSession = async () => {
    if (!activeSession || !activeConversation || !replyInput.trim() || sessionSending) return;
    const prompt = replyInput.trim();
    setReplyInput("");
    await runConversationAgent({
      state,
      setState,
      setRuns,
      conversationId: activeConversation.id,
      prompt,
      onDone: (content) => sendWeixinText(activeSession.externalUserId, content, activeSession.contextToken)
    });
  };

  const stopSessionRun = async () => {
    if (!activeSession || !activeConversation || !activeRun || !sessionSending) return;
    await stopChatStream(activeRun.runId);
    const assistant: ChatMessage = { id: crypto.randomUUID(), role: "assistant", content: "已停止本轮 IM Agent 执行。", timestamp: new Date().toISOString() };
    setRuns((current) => ({
      ...current,
      [activeConversation.id]: {
        ...activeRun,
        phase: "stopped",
        message: "用户已停止本轮 IM Agent 执行。",
        todos: activeRun.todos.map((todo) => todo.status === "completed" ? todo : { ...todo, status: "cancelled" })
      }
    }));
    setState((current) => ({
      ...current,
      tasks: current.tasks.map((task) => task.conversationId === activeConversation.id && ["todo", "doing", "review", "paused"].includes(task.status) ? { ...task, status: "stopped" } : task),
      conversations: current.conversations.map((item) => item.id === activeConversation.id ? { ...item, messages: [...item.messages, assistant], updatedAt: new Date().toISOString() } : item)
    }));
    await sendWeixinText(activeSession.externalUserId, assistant.content, activeSession.contextToken);
  };

  return (
    <>
      <PageHeader eyebrow="IM Gateway" title="IM 渠道连接" description="渠道连接后会在独立会话里接入小亨 Agent，消息、todo 和工具调用轨迹会持续显示。" />
      <section className="market-toolbar im-toolbar">
        <div className="tabs">
          <button className={tab === "channels" ? "active" : ""} onClick={() => setTab("channels")}>渠道连接</button>
          <button className={tab === "sessions" ? "active" : ""} onClick={() => setTab("sessions")}>渠道会话</button>
        </div>
        <button className="secondary" onClick={() => void startWeixinMonitor().then(refresh)}><Network size={16} />启动监听</button>
      </section>
      {message && <section className="panel im-message">{message}</section>}
      {tab === "channels" && (
        <>
          {weixinLogin.qrcodeUrl && weixinLogin.status !== "connected" && <section className="panel weixin-qr-panel">
            <div>
              <h3>微信扫码连接</h3>
              <p>{weixinLogin.message ?? "请使用微信扫码，并在手机端确认登录。"}</p>
            </div>
            <QRCodeSVG value={weixinLogin.qrcodeUrl} size={180} includeMargin />
          </section>}
          <div className="channel-grid">
            {channels.map((channel) => (
              <section className="channel-card" key={channel.id}>
                <div className="channel-head">
                  <Link2 size={19} />
                  <span className={`status-badge ${channel.status}`}>{channel.status === "connected" ? "已连接" : channel.status === "configured" ? "已配置" : "未连接"}</span>
                </div>
                <h3>{channel.name}</h3>
                <p>{channel.description}</p>
                {channel.setupHint && <div className="setup-hint">{channel.setupHint}</div>}
                <div className="cap-row">{channel.capabilities.map((cap) => <span key={cap}>{cap}</span>)}</div>
                {channel.connectionMode !== "xiaoheng-weixin-native" && <div className="channel-config">
                  {(channel.configSchema ?? []).map((field) => {
                    const value = drafts[channel.id]?.[field.key] ?? String(channel.config?.[field.key] ?? "");
                    return <label key={field.key}>{field.label}<input type={field.secret ? "password" : "text"} value={value} placeholder={field.placeholder} onChange={(event) => updateDraft(channel, field.key, event.target.value)} /></label>;
                  })}
                </div>}
                <div className="channel-actions">
                  {channel.connectionMode !== "xiaoheng-weixin-native" && <button className="secondary" onClick={() => void saveConfig(channel)} disabled={busy === `save-${channel.id}`}>{busy === `save-${channel.id}` ? <LoaderCircle className="spin" size={16} /> : <Save size={16} />}保存配置</button>}
                  <button className="secondary" onClick={() => void testChannel(channel)} disabled={busy === `test-${channel.id}`}>{busy === `test-${channel.id}` ? <LoaderCircle className="spin" size={16} /> : <Send size={16} />}测试</button>
                  <button className={channel.status === "connected" ? "secondary" : "primary"} onClick={() => void toggle(channel)} disabled={busy === channel.id}>{busy === channel.id ? <LoaderCircle className="spin" size={16} /> : <CircleDot size={16} />}{channel.status === "connected" ? "断开" : channel.connectionMode === "xiaoheng-weixin-native" ? "扫码连接" : "连接"}</button>
                </div>
              </section>
            ))}
          </div>
        </>
      )}
      {tab === "sessions" && (
        <div className="im-session-layout">
          <aside className="panel im-session-list">
            <strong>渠道会话</strong>
            {state.imSessions.length === 0 && <p>暂无渠道消息。微信扫码连接后，收到的新消息会自动出现在这里。</p>}
            {state.imSessions.map((session) => (
              <button key={session.id} className={`im-session-item ${activeSession?.id === session.id ? "active" : ""}`} onClick={() => selectSession(session.id)}>
                <MessageCircle size={15} />
                <span>{session.title}</span>
                <small>{session.channel} · {new Date(session.lastMessageAt).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</small>
                {!!session.unread && <b>{session.unread}</b>}
              </button>
            ))}
          </aside>
          <section className="chat-window im-chat-window">
            {activeConversation ? (
              <>
                <div className="im-chat-head">
                  <div>
                    <span className="eyebrow">{activeSession?.channel ?? "IM"}</span>
                    <h3>{activeSession?.title}</h3>
                    <p>{activeSession?.externalUserId}</p>
                  </div>
                  <span className={`status-badge ${sessionSending ? "testing" : "ready"}`}>{sessionSending ? "处理中" : "可对话"}</span>
                </div>
                <div className="chat-messages">
                  {activeConversation.messages.map((item) => (
                    <div key={item.id} className={`message ${item.role}`}>
                      <div className="message-avatar">{item.role === "assistant" ? avatar(state.agents[0], "sm") : <span className="avatar avatar-sm user-avatar">微</span>}</div>
                      <div className="bubble">
                        <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.content}</ReactMarkdown>
                        <time>{new Date(item.timestamp).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })}</time>
                      </div>
                    </div>
                  ))}
                  {sessionSending && (
                    <div className="message assistant">
                      <div className="message-avatar">{avatar(state.agents[0], "sm")}</div>
                      <div className="bubble typing"><i /><i /><i /></div>
                    </div>
                  )}
                </div>
                <RunPanel run={activeRun} sending={sessionSending} />
                <div className="composer im-composer">
                  <textarea value={replyInput} onChange={(event) => setReplyInput(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void sendToActiveSession(); } }} placeholder="在当前微信会话内继续让小亨处理..." />
                  <div className="composer-options">
                    {sessionSending && <button className="stop-button" onClick={() => void stopSessionRun()} title="停止"><Square size={15} />停止</button>}
                    <button className="send-button" onClick={() => void sendToActiveSession()} disabled={!replyInput.trim() || sessionSending} title="发送"><Send size={17} /></button>
                  </div>
                </div>
              </>
            ) : (
              <div className="im-empty panel">
                <MessageCircle size={28} />
                <h3>等待渠道消息</h3>
                <p>微信渠道收到用户消息后，小亨会自动创建会话、执行任务并把回复发回微信。</p>
              </div>
            )}
          </section>
        </div>
      )}
    </>
  );
}

function RuntimePage() {
  const [topology, setTopology] = useState<RuntimeTopology | null>(null);
  useEffect(() => {
    getRuntimeTopology().then(setTopology).catch((error) => console.error(error));
  }, []);

  return (
    <>
      <PageHeader eyebrow="Runtime" title="运行时工具" description="展示当前可供 Agent 使用的模块和工具注册状态。" />
      <div className="runtime-grid">
        <section className="panel">
          <div className="panel-title"><h2>模块</h2></div>
          {topology?.modules.map((module) => (
            <div className="runtime-row" key={module.id}>
              <span className={`status-badge ${module.status}`}>{module.status}</span>
              <div><strong>{module.name}</strong><small>{module.description}</small></div>
            </div>
          ))}
        </section>
        <section className="panel">
          <div className="panel-title"><h2>注册工具</h2></div>
          {topology?.tools.map((tool) => (
            <div className="runtime-row" key={tool.id}>
              <span className="chip">{tool.category}</span>
              <div><strong>{tool.name}</strong><small>{tool.description}</small></div>
            </div>
          ))}
        </section>
      </div>
    </>
  );
}

function MemoryPage({ state, setState }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> }) {
  const [text, setText] = useState("");
  const [memoryStatus, setMemoryStatus] = useState("");
  const appendToMemoryFile = async (title: string, content: string) => {
    if (!state.settings.memoryFileEnabled) return;
    try {
      const result = await appendMemoryFile(state.settings, title, content);
      const indexStatus = result.indexOk === false ? "索引稍后重建" : "索引已更新";
      setMemoryStatus(`已同步 MEMORY.md：${result.charCount} 字符，${result.byteSize} 字节；${indexStatus}。`);
    } catch (error) {
      setMemoryStatus(`MEMORY.md 同步失败：${error instanceof Error ? error.message : String(error)}`);
    }
  };
  const runDream = () => {
    const activeConversation = state.conversations.find((item) => item.id === state.activeConversationId);
    const recent = activeConversation?.messages.slice(-8).map((item) => `${item.role}: ${item.content}`).join("\n").slice(0, 1200) ?? "";
    const activeTasks = state.tasks.filter((task) => task.status !== "done").slice(0, 6).map((task) => `${task.title}(${statusLabel(task.status)})`).join("；");
    const content = [
      recent ? `近期对话：${recent}` : "",
      activeTasks ? `未完成任务：${activeTasks}` : "",
      `项目组数量：${state.offices.length}；启用技能：${state.skills.filter((skill) => skill.enabled).length}。`
    ].filter(Boolean).join("\n");
    const title = `Dream 记忆整理 ${new Date().toLocaleString("zh-CN")}`;
    const finalContent = content || "当前没有足够的新上下文可整理。";
    setState((current) => ({
      ...current,
      memories: [{
        id: `memory-dream-${crypto.randomUUID()}`,
        title,
        content: finalContent,
        createdAt: new Date().toISOString().slice(0, 10),
        kind: "dream",
        confidence: Math.max(0, Math.min(100, state.settings.dreamMinConfidence)),
        source: activeConversation?.id ?? "manual",
        lastConfirmedAt: new Date().toISOString()
      }, ...current.memories]
    }));
    void appendToMemoryFile(title, finalContent);
  };
  const saveManualMemory = () => {
    const content = text.trim();
    if (!content) return;
    const title = "用户记忆";
    setState((current) => ({
      ...current,
      memories: [...current.memories, { id: crypto.randomUUID(), title, content, createdAt: new Date().toISOString().slice(0, 10), kind: "manual", confidence: 100 }]
    }));
    setText("");
    void appendToMemoryFile(title, content);
  };
  return (
    <>
      <PageHeader eyebrow="Memory" title="长期记忆" description="保存长期有效的事实、偏好、规则和项目决策。" action={<button className="primary" onClick={runDream}><Lightbulb size={16} />梦境整理</button>} />
      <div className="memory-layout">
        <section className="panel">
          {state.memories.map((memory) => (
            <article className="memory-row" key={memory.id}>
              <Brain size={18} />
              <div><h3>{memory.title}</h3><p>{memory.content}</p><time>{memory.createdAt}{memory.kind ? ` · ${memory.kind}` : ""}{memory.confidence ? ` · 置信度 ${memory.confidence}%` : ""}</time></div>
              <button className="icon-btn flat" onClick={() => setState((current) => ({ ...current, memories: current.memories.filter((item) => item.id !== memory.id) }))}><Trash2 size={15} /></button>
            </article>
          ))}
        </section>
        <section className="panel add-panel">
          <h2>新增记忆</h2>
          {memoryStatus && <p className="form-hint">{memoryStatus}</p>}
          <textarea value={text} onChange={(event) => setText(event.target.value)} placeholder="例如：所有对外报告优先使用简体中文。" />
          <button className="primary" onClick={saveManualMemory}>
            <Save size={16} />
            保存
          </button>
        </section>
      </div>
    </>
  );
}

function SchedulePage({ state, setState, openModal }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; openModal: (modal: ModalState) => void }) {
  return (
    <>
      <PageHeader eyebrow="Automation" title="自动化" description="让小亨或指定人员按计划触发任务、整理记忆和执行工作流。" action={<button className="primary" onClick={() => openModal({ type: "schedule" })}><Plus size={17} />新建自动化</button>} />
      <section className="panel schedule-list">
        {state.schedules.map((item) => (
          <div className="schedule-row" key={item.id}>
            <CalendarDays size={20} />
            <div><h3>{item.name}</h3><p>{item.description}</p><span>{item.mode ?? "cron"} · {item.time ?? item.cron} · {state.agents.find((agent) => agent.id === item.executorId)?.name ?? "小亨自动执行"}</span></div>
            <label className="switch"><input type="checkbox" checked={item.enabled} onChange={() => setState((current) => ({ ...current, schedules: current.schedules.map((schedule) => schedule.id === item.id ? { ...schedule, enabled: !schedule.enabled } : schedule) }))} /><i /></label>
            <span className="row-actions"><button onClick={() => openModal({ type: "schedule", item })}>编辑</button><button onClick={() => setState((current) => ({ ...current, schedules: current.schedules.filter((schedule) => schedule.id !== item.id) }))}>删除</button></span>
          </div>
        ))}
      </section>
    </>
  );
}

function SettingsPage({ state, setState }: { state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>> }) {
  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState("");
  const [modelChecking, setModelChecking] = useState(false);
  const [draft, setDraft] = useState<AppState["settings"]>(state.settings);
  const archivedTasks = state.tasks.filter((task) => task.archived).sort((a, b) => Date.parse(b.archivedAt ?? "") - Date.parse(a.archivedAt ?? ""));
  const dirty = JSON.stringify(draft) !== JSON.stringify(state.settings);
  useEffect(() => setDraft(state.settings), [state.settings]);
  const update = (key: keyof AppState["settings"], value: string | number | boolean | ModelOption[] | string[]) => setDraft((current) => ({ ...current, [key]: value }));
  const updateProvider = async (provider: AppState["settings"]["provider"]) => {
    const configured = await hasApiKey(provider);
    const preset = providerPreset(provider);
    setDraft((current) => ({ ...current, provider, baseUrl: provider === "compatible" ? current.baseUrl : preset.baseUrl, apiKeyConfigured: configured }));
  };
  const toggleModel = (modelId: string, checked: boolean) => {
    setDraft((current) => {
      const next = checked ? Array.from(new Set([...current.enabledModels, modelId])) : current.enabledModels.filter((id) => id !== modelId);
      const enabledModels = next.length ? next : [current.model || modelId];
      const model = enabledModels.includes(current.model) ? current.model : enabledModels[0];
      return { ...current, enabledModels, model };
    });
  };
  const validateProvider = async () => {
    setModelChecking(true);
    try {
      const result = await validateModelProvider(draft, apiKey);
      const models = result.models;
      const preferred = models.find((model) => model.id === draft.model)?.id ?? models.find((model) => model.category === "chat" || model.category === "reasoning" || model.category === "coder")?.id ?? models[0]?.id ?? draft.model;
      setDraft((current) => ({
        ...current,
        provider: result.provider,
        baseUrl: result.baseUrl,
        availableModels: models,
        enabledModels: current.enabledModels.filter((id) => models.some((model) => model.id === id)).length ? current.enabledModels.filter((id) => models.some((model) => model.id === id)) : [preferred],
        model: preferred,
        apiKeyConfigured: true
      }));
      if (apiKey.trim()) setApiKey("");
      setMessage(result.message);
    } catch (error) {
      setMessage(String(error));
    } finally {
      setModelChecking(false);
    }
  };
  const saveSettings = async () => {
    const next = { ...state, settings: draft };
    setState(next);
    await saveState(next);
    setMessage("设置已保存，并会应用到下一轮对话。");
  };
  const saveKey = async () => {
    try {
      const ok = await saveApiKey(apiKey, draft.provider);
      update("apiKeyConfigured", ok);
      setApiKey("");
      setMessage(ok ? "API Key 已保存，可供当前会话和后端读取。" : "API Key 保存失败。");
    } catch (error) {
      update("apiKeyConfigured", false);
      setMessage(String(error));
    }
  };

  return (
    <>
      <PageHeader eyebrow="Settings" title="应用设置" description="配置模型服务、Agent 执行、工具权限和梦境记忆。" action={<div className="settings-save">{dirty && <span>有未保存的更改</span>}<button className="primary" onClick={() => void saveSettings()}><Save size={16} />保存设置</button></div>} />
      <div className="settings-grid settings-wide">
        <section className="panel settings-card">
          <div className="settings-title"><Bot size={22} /><div><h2>AI 模型</h2><p>全局默认模型；专家也会优先读取这里的上下文、输出和流式配置。</p></div></div>
          <label>AI 提供商<select value={draft.provider} onChange={(event) => void updateProvider(event.target.value as AppState["settings"]["provider"])}>{providerPresets.map((item) => <option value={item.id} key={item.id}>{item.icon} {item.label}</option>)}</select></label>
          <label>BASE URL<input value={draft.baseUrl} onChange={(event) => update("baseUrl", event.target.value)} /></label>
          <label>API KEY<div className="inline-input"><input type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} placeholder={draft.apiKeyConfigured ? "已保存，可输入新密钥替换" : "输入 API Key"} /><button onClick={() => void saveKey()}>保存密钥</button><button className="secondary" onClick={() => void validateProvider()} disabled={modelChecking}>{modelChecking ? <LoaderCircle className="spin" size={15} /> : <ShieldCheck size={15} />}验证并拉取模型</button></div></label>
          <label>默认模型<select value={draft.model} onChange={(event) => update("model", event.target.value)}>{selectableChatModels(draft).map((model) => <option value={model.id} key={model.id}>{modelIcon(model.id, draft.availableModels)} {model.name}</option>)}</select></label>
          <div className="settings-box model-market">
            <strong>可用模型</strong>
            <p className="form-hint">{providerPreset(draft.provider).icon} {providerPreset(draft.provider).label} · {providerPreset(draft.provider).hint}</p>
            <div className="model-category-grid">
              {Object.entries(modelCategoryLabels).map(([key, label]) => {
                const models = availableProviderModels(draft).filter((model) => model.category === key);
                if (!models.length) return null;
                return (
                  <section key={key}>
                    <h4>{label}</h4>
                    <div className="model-option-grid">
                      {models.map((model) => (
                        <label className={`model-option ${draft.enabledModels.includes(model.id) ? "active" : ""}`} key={model.id}>
                          <input type="checkbox" checked={draft.enabledModels.includes(model.id)} onChange={(event) => toggleModel(model.id, event.target.checked)} />
                          <span>{modelIcon(model.id, draft.availableModels)}</span>
                          <b>{model.name}</b>
                        </label>
                      ))}
                    </div>
                  </section>
                );
              })}
            </div>
          </div>
          <label>单次响应最大 TOKEN 数<input type="number" value={draft.maxOutputTokens} onChange={(event) => update("maxOutputTokens", Number(event.target.value))} /></label>
          <label>上下文窗口大小 TOKEN 数<input type="number" value={draft.contextWindowTokens} onChange={(event) => update("contextWindowTokens", Number(event.target.value))} /></label>
          <label className="check-line"><input type="checkbox" checked={draft.streamOutput} onChange={(event) => update("streamOutput", event.target.checked)} />启用 token 级流式输出</label>
          <div className="settings-box"><strong>Vision 模型</strong><label className="check-line"><input type="checkbox" checked={draft.visionUseMainModel} onChange={(event) => update("visionUseMainModel", event.target.checked)} />使用主 LLM 作为 VISION 模型</label><label className="check-line"><input type="checkbox" checked={draft.visionEnabled} onChange={(event) => update("visionEnabled", event.target.checked)} />模型支持视觉</label><p className="form-hint">{draft.visionEnabled ? "Vision 能力已启用。" : "Vision 模型未配置，UIA 和截图工具会优先作为普通文件/工具结果处理。"}</p></div>
          <p className={draft.apiKeyConfigured ? "form-ok" : "form-hint"}>{draft.provider === "ollama" ? "Ollama 不需要 API Key。" : draft.apiKeyConfigured ? "API Key 已配置。" : "尚未保存 API Key。"}</p>
          {message && <p className="form-hint">{message}</p>}
        </section>
        <section className="panel settings-card">
          <div className="settings-title"><Bot size={22} /><div><h2>Agent</h2><p>控制 Agent 单次任务的工具迭代、工作空间和安全确认策略。</p></div></div>
          <label>工作区根目录<input value={draft.workspaceRoot} onChange={(event) => update("workspaceRoot", event.target.value)} placeholder="留空使用当前用户文档目录下的 XiaohengWorkBuddy" /></label>
          <label className="check-line"><input type="checkbox" checked={draft.allowOutsideWorkspace} onChange={(event) => update("allowOutsideWorkspace", event.target.checked)} />允许访问工作区以外的文件</label>
          <label>最大迭代次数<input type="number" value={draft.agentMaxIterations} onChange={(event) => update("agentMaxIterations", Number(event.target.value))} /></label>
          <label>自动压缩累计输入阈值 TOKEN<input type="number" value={draft.compressionTokenThreshold} onChange={(event) => update("compressionTokenThreshold", Number(event.target.value))} /></label>
          <div className="settings-box"><strong>上下文压缩三档阈值（%）</strong><div className="settings-3col"><label>MICRO<input type="number" value={draft.compressionMicroPercent} onChange={(event) => update("compressionMicroPercent", Number(event.target.value))} /></label><label>AUTO<input type="number" value={draft.compressionAutoPercent} onChange={(event) => update("compressionAutoPercent", Number(event.target.value))} /></label><label>FULL<input type="number" value={draft.compressionFullPercent} onChange={(event) => update("compressionFullPercent", Number(event.target.value))} /></label></div></div>
          <div className="settings-2col"><label>单条工具结果上限 TOKEN<input type="number" value={draft.toolResultTokenLimit} onChange={(event) => update("toolResultTokenLimit", Number(event.target.value))} /></label><label>摘要模型覆盖<input value={draft.summaryModel} onChange={(event) => update("summaryModel", event.target.value)} placeholder="可选；留空使用主模型" /></label><label>LLM 读取超时（秒）<input type="number" value={draft.llmTimeoutSeconds} onChange={(event) => update("llmTimeoutSeconds", Number(event.target.value))} /></label><label>专家任务超时（秒）<input type="number" value={draft.expertTaskTimeoutSeconds} onChange={(event) => update("expertTaskTimeoutSeconds", Number(event.target.value))} /></label></div>
          <label className="check-line"><input type="checkbox" checked={draft.confirmShell} onChange={(event) => update("confirmShell", event.target.checked)} />确认 Shell 命令</label>
          <label className="check-line"><input type="checkbox" checked={draft.confirmFileWrite} onChange={(event) => update("confirmFileWrite", event.target.checked)} />确认文件写入</label>
          <label className="check-line"><input type="checkbox" checked={draft.browserBackground} onChange={(event) => update("browserBackground", event.target.checked)} />浏览器后台运行</label>
          <label>界面主题<select value={draft.theme} onChange={(event) => update("theme", event.target.value)}><option value="light">明亮</option><option value="dark">深色</option></select></label>
          <div className="setting-line"><div><strong>本地优先</strong><span>任务、记忆和配置优先保存在本机。</span></div><span className="safe-badge"><ShieldCheck size={14} />已启用</span></div>
        </section>
        <section className="panel settings-card settings-span">
          <div className="settings-title"><Activity size={22} /><div><h2>Heartbeat 心跳</h2><p>参考 OpenClaw Gateway 的 Heartbeat 思路，按固定间隔触发一次轻量巡检，用于自动工作、自动化推进、暂停任务恢复和轮询。</p></div></div>
          <div className="settings-3col">
            <label className="check-line"><input type="checkbox" checked={draft.heartbeatEnabled} onChange={(event) => update("heartbeatEnabled", event.target.checked)} />启用心跳</label>
            <label>频率（分钟）<input type="number" min={5} value={draft.heartbeatIntervalMinutes} onChange={(event) => update("heartbeatIntervalMinutes", Number(event.target.value))} /></label>
            <label>执行模型<select value={draft.heartbeatModel} onChange={(event) => update("heartbeatModel", event.target.value)}><option value="">跟随全局默认模型 · {draft.model}</option>{selectableChatModels(draft).map((model) => <option value={model.id} key={model.id}>{modelIcon(model.id, draft.availableModels)} {model.name}</option>)}</select></label>
          </div>
          <p className="form-hint">默认每 60 分钟触发一次。没有待处理事项时 Agent 会回复 HEARTBEAT_OK 并保持静默记录；有自动化或暂停任务时会进入任务中心流程。</p>
          <p className="form-hint">上次心跳：{draft.heartbeatLastRun ? new Date(draft.heartbeatLastRun).toLocaleString("zh-CN") : "尚未运行"}</p>
        </section>
        <section className="panel settings-card settings-span">
          <div className="settings-title"><Lightbulb size={22} /><div><h2>梦境记忆</h2><p>参考 OpenClaw 的 Dream/Memory consolidation 思路，在空闲或手动触发时整理对话、任务和项目事实，沉淀为长期记忆。</p></div></div>
          <div className="settings-3col"><label className="check-line"><input type="checkbox" checked={draft.dreamEnabled} onChange={(event) => update("dreamEnabled", event.target.checked)} />启用梦境</label><label className="check-line"><input type="checkbox" checked={draft.dreamAutoRun} onChange={(event) => update("dreamAutoRun", event.target.checked)} />自动整理</label><label>最低置信度<input type="number" value={draft.dreamMinConfidence} onChange={(event) => update("dreamMinConfidence", Number(event.target.value))} /></label><label>整理间隔（小时）<input type="number" value={draft.dreamIntervalHours} onChange={(event) => update("dreamIntervalHours", Number(event.target.value))} /></label><label>保留天数<input type="number" value={draft.dreamRetentionDays} onChange={(event) => update("dreamRetentionDays", Number(event.target.value))} /></label></div>
          <div className="settings-box"><strong>长期记忆文件</strong><label className="check-line"><input type="checkbox" checked={draft.memoryFileEnabled} onChange={(event) => update("memoryFileEnabled", event.target.checked)} />使用 MEMORY.md 作为长期记忆源</label><label className="check-line"><input type="checkbox" checked={draft.memoryEmbeddingEnabled} onChange={(event) => update("memoryEmbeddingEnabled", event.target.checked)} />启用 Embedding 混合检索</label><label>Embedding 模型<select value={draft.memoryEmbeddingModel} onChange={(event) => update("memoryEmbeddingModel", event.target.value)}><option value="">仅关键词检索</option>{selectableEmbeddingModels(draft).map((model) => <option value={model.id} key={model.id}>{modelIcon(model.id, draft.availableModels)} {model.name}</option>)}</select></label><label>手动模型 ID<input value={draft.memoryEmbeddingModel} onChange={(event) => update("memoryEmbeddingModel", event.target.value)} placeholder="例如 text-embedding-3-small；留空使用关键词检索" /></label><p className="form-hint">启用后会维护 memory-index.json；记忆写入或搜索时增量更新索引，Embedding 不可用时自动退回关键词检索。</p></div>
        </section>
        <section className="panel settings-card settings-span">
          <div className="settings-title"><Archive size={22} /><div><h2>已归档任务</h2><p>任务中心归档后的交付树只在这里查看，避免干扰当前执行看板。</p></div></div>
          <div className="archive-list">
            {archivedTasks.length === 0 && <p className="form-hint">暂无归档任务。</p>}
            {archivedTasks.map((task) => (
              <article className="archive-row" key={task.id}>
                <div><strong>{task.title}</strong><small>{task.archivedAt?.slice(0, 19).replace("T", " ")} · {state.agents.find((agent) => agent.id === task.ownerId)?.name ?? "未指定"}</small><p>{task.description}</p></div>
                <button className="secondary" onClick={() => setState((current) => ({ ...current, tasks: current.tasks.map((item) => item.id === task.id ? { ...item, archived: false, archivedAt: undefined } : item) }))}>恢复到任务中心</button>
              </article>
            ))}
          </div>
        </section>
      </div>
    </>
  );
}

function TaskRow({ task, agent }: { task: WorkTask; agent?: Agent }) {
  return (
    <div className="focus-row">
      <span className={`task-state state-${task.status}`}>{task.status === "doing" ? <LoaderCircle size={15} /> : task.status === "review" ? <Activity size={15} /> : <Clock3 size={15} />}</span>
      <div><strong>{task.title}</strong><small>{agent?.name} · {task.dueDate}</small></div>
      <span className={`priority small ${task.priority}`}>{priorityLabel(task.priority)}</span>
    </div>
  );
}

function EditorModal({ modal, state, setState, close }: { modal: ModalState; state: AppState; setState: React.Dispatch<React.SetStateAction<AppState>>; close: () => void }) {
  const agent = modal.type === "agent" ? modal.item : undefined;
  const office = modal.type === "office" ? modal.item : undefined;
  const task = modal.type === "task" ? modal.item : undefined;
  const skill = modal.type === "skill" ? modal.item : undefined;
  const schedule = modal.type === "schedule" ? modal.item : undefined;

  const [name, setName] = useState(agent?.name ?? office?.name ?? skill?.name ?? schedule?.name ?? "");
  const [title, setTitle] = useState(agent?.title ?? task?.title ?? "");
  const [description, setDescription] = useState(office?.description ?? task?.description ?? skill?.description ?? schedule?.description ?? "");
  const [role, setRole] = useState<AgentRole>(agent?.role ?? "har");
  const [agentModel, setAgentModel] = useState(agent?.defaultModel ?? "");
  const [taskStatus, setTaskStatus] = useState<TaskStatus>(task?.status ?? "todo");
  const [priority, setPriority] = useState<WorkTask["priority"]>(task?.priority ?? "medium");
  const [owner, setOwner] = useState(task?.ownerId ?? state.agents.find((item) => item.role !== "xiaoheng")?.id ?? "xiaoheng");
  const [officeId, setOfficeId] = useState(task?.officeId ?? state.offices[0]?.id ?? "");
  const [dueDate, setDueDate] = useState(task?.dueDate ?? new Date(Date.now() + 7 * 86400000).toISOString().slice(0, 10));
  const [category, setCategory] = useState(skill?.category ?? "办公");
  const [enabled, setEnabled] = useState(skill?.enabled ?? schedule?.enabled ?? true);
  const [cron, setCron] = useState(schedule?.cron ?? "0 4 * * *");
  const [scheduleMode, setScheduleMode] = useState<ScheduleItem["mode"]>(schedule?.mode ?? "daily");
  const [scheduleTime, setScheduleTime] = useState(schedule?.time ?? "04:00");
  const [scheduleExecutor, setScheduleExecutor] = useState(schedule?.executorId ?? "xiaoheng");
  const [scheduleAction, setScheduleAction] = useState<ScheduleItem["action"]>(schedule?.action ?? "dream_memory");
  const [schedulePrompt, setSchedulePrompt] = useState(schedule?.prompt ?? "整理最近对话、任务、项目和用户偏好，形成可复用的长期记忆。");
  const [progress, setProgress] = useState(String(office?.progress ?? 0));
  const [color, setColor] = useState(agent?.color ?? "#2563eb");
  const [specialty, setSpecialty] = useState(agent?.specialty ?? "");
  const [officeManagers, setOfficeManagers] = useState<string[]>(office?.managerIds ?? state.agents.filter((item) => item.role === "manager").map((item) => item.id));
  const [officeMembers, setOfficeMembers] = useState<string[]>(office?.memberIds ?? state.agents.filter((item) => item.role === "har").map((item) => item.id));

  const save = () => {
    if (modal.type === "agent") {
      if (!name.trim()) return;
      const id = agent?.id ?? `agent-${crypto.randomUUID()}`;
      const item: Agent = { id, name: name.trim(), role, title: title.trim() || roleLabel(role), specialty: specialty.trim() || "待补充职责", status: agent?.status ?? "idle", color, defaultModel: agentModel || undefined };
      setState((current) => ({ ...current, agents: upsertById(current.agents, [item]) }));
    }
    if (modal.type === "office") {
      if (!name.trim()) return;
      const id = office?.id ?? `office-${crypto.randomUUID()}`;
      const item: Office = { id, name: name.trim(), description: description.trim() || "新的项目组", progress: Math.max(0, Math.min(100, Number(progress) || 0)), managerIds: officeManagers, memberIds: officeMembers, goals: office?.goals ?? [], deliverables: office?.deliverables ?? [], plan: office?.plan ?? [] };
      setState((current) => ({ ...current, offices: upsertById(current.offices, [item]) }));
    }
    if (modal.type === "task") {
      if (!title.trim()) return;
      const item: WorkTask = { id: task?.id ?? crypto.randomUUID(), officeId, title: title.trim(), description: description.trim() || "待补充任务说明", ownerId: owner, status: taskStatus, priority, dueDate, conversationId: task?.conversationId };
      setState((current) => ({ ...current, tasks: upsertById(current.tasks, [item]) }));
    }
    if (modal.type === "skill") {
      if (!name.trim()) return;
      const item: SkillItem = { id: skill?.id ?? `skill-${crypto.randomUUID()}`, name: name.trim(), description: description.trim() || "新的工作技能", enabled, category: category.trim() || "办公", source: skill?.source ?? "builtin", tools: skill?.tools ?? [] };
      setState((current) => ({ ...current, skills: upsertById(current.skills, [item]) }));
    }
    if (modal.type === "schedule") {
      if (!name.trim()) return;
      const item: ScheduleItem = { id: schedule?.id ?? `schedule-${crypto.randomUUID()}`, name: name.trim(), description: description.trim() || "自动化任务", cron: cron.trim() || "0 4 * * *", mode: scheduleMode, time: scheduleTime, executorId: scheduleExecutor, action: scheduleAction, prompt: schedulePrompt, enabled };
      setState((current) => ({ ...current, schedules: upsertById(current.schedules, [item]) }));
    }
    close();
  };

  const heading = modal.type === "agent" ? (agent ? "编辑成员" : "新增成员") : modal.type === "office" ? (office ? "编辑项目组" : "新建项目组") : modal.type === "task" ? (task ? "编辑任务" : "新建任务") : modal.type === "skill" ? (skill ? "编辑技能" : "新增技能") : (schedule ? "编辑自动化" : "新建自动化");

  return (
    <motion.div className="modal-backdrop" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={close}>
      <motion.div className="modal" initial={{ scale: 0.96, y: 18 }} animate={{ scale: 1, y: 0 }} onMouseDown={(event) => event.stopPropagation()}>
        <header><div><span className="eyebrow">Editor</span><h2>{heading}</h2></div><button className="icon-btn flat" onClick={close}><X size={18} /></button></header>
        {modal.type !== "task" && <label>名称<input autoFocus value={name} onChange={(event) => setName(event.target.value)} /></label>}
        {modal.type === "agent" && <><label>角色<select value={role} onChange={(event) => setRole(event.target.value as AgentRole)} disabled={agent?.role === "xiaoheng"}><option value="manager">经理</option><option value="har">职员</option><option value="xiaoheng">小亨</option></select></label><label>头衔<input value={title} onChange={(event) => setTitle(event.target.value)} /></label><label>专长<textarea value={specialty} onChange={(event) => setSpecialty(event.target.value)} /></label><label>默认模型<select value={agentModel} onChange={(event) => setAgentModel(event.target.value)}><option value="">跟随全局默认模型 · {state.settings.model}</option>{selectableChatModels(state.settings).map((model) => <option value={model.id} key={model.id}>{modelIcon(model.id, state.settings.availableModels)} {model.name}</option>)}</select></label><p className="form-hint">人员状态由任务中心实时推导，不再手动设置。</p><label>颜色<input type="color" value={color} onChange={(event) => setColor(event.target.value)} /></label></>}
        {modal.type === "office" && <><label>描述<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label><label>进度<input type="number" min="0" max="100" value={progress} onChange={(event) => setProgress(event.target.value)} /></label><div className="member-picker"><span>经理</span>{state.agents.filter((item) => item.role === "manager").map((item) => <label key={item.id}><input type="checkbox" checked={officeManagers.includes(item.id)} onChange={(event) => setOfficeManagers((current) => event.target.checked ? [...current, item.id] : current.filter((id) => id !== item.id))} />{item.name}</label>)}</div><div className="member-picker"><span>职员</span>{state.agents.filter((item) => item.role === "har").map((item) => <label key={item.id}><input type="checkbox" checked={officeMembers.includes(item.id)} onChange={(event) => setOfficeMembers((current) => event.target.checked ? [...current, item.id] : current.filter((id) => id !== item.id))} />{item.name}</label>)}</div></>}
        {modal.type === "task" && <><label>任务名称<input autoFocus value={title} onChange={(event) => setTitle(event.target.value)} /></label><label>任务说明<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label><label>项目组<select value={officeId} onChange={(event) => setOfficeId(event.target.value)}><option value="">不使用项目</option>{state.offices.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></label><label>负责人<select value={owner} onChange={(event) => setOwner(event.target.value)}>{state.agents.map((item) => <option value={item.id} key={item.id}>{item.name} · {item.title}</option>)}</select></label><label>优先级<select value={priority} onChange={(event) => setPriority(event.target.value as WorkTask["priority"])}><option value="high">高</option><option value="medium">中</option><option value="low">低</option></select></label><label>状态<select value={taskStatus} onChange={(event) => setTaskStatus(event.target.value as TaskStatus)}><option value="todo">计划中</option><option value="doing">进行中</option><option value="review">待验收</option><option value="done">已交付</option><option value="paused">已暂停</option><option value="stopped">已停止</option></select></label><label>截止日期<input type="date" value={dueDate} onChange={(event) => setDueDate(event.target.value)} /></label></>}
        {modal.type === "skill" && <><label>分类<input value={category} onChange={(event) => setCategory(event.target.value)} /></label><label>说明<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label><label className="check-line"><input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />启用</label></>}
        {modal.type === "schedule" && <><label>简介<textarea value={description} onChange={(event) => setDescription(event.target.value)} /></label><label>执行方式<select value={scheduleMode} onChange={(event) => setScheduleMode(event.target.value as ScheduleItem["mode"])}><option value="daily">每天定时</option><option value="cron">Cron 表达式</option><option value="manual">仅手动触发</option></select></label><label>设定时间<input type="time" value={scheduleTime} onChange={(event) => setScheduleTime(event.target.value)} /></label><label>Cron 表达式<input value={cron} onChange={(event) => setCron(event.target.value)} /></label><label>执行人员<select value={scheduleExecutor} onChange={(event) => setScheduleExecutor(event.target.value)}>{state.agents.map((item) => <option value={item.id} key={item.id}>{item.name} · {item.title}</option>)}</select></label><label>执行动作<select value={scheduleAction} onChange={(event) => setScheduleAction(event.target.value as ScheduleItem["action"])}><option value="dream_memory">整理归纳记忆</option><option value="agent_prompt">发送 Agent 提示</option><option value="custom">自定义工作流</option></select></label><label>执行提示词<textarea value={schedulePrompt} onChange={(event) => setSchedulePrompt(event.target.value)} /></label><label className="check-line"><input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />启用</label></>}
        <footer><button className="secondary" onClick={close}>取消</button><button className="primary" onClick={save}>保存</button></footer>
      </motion.div>
    </motion.div>
  );
}

export default App;
