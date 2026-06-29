export type AgentRole = "xiaoheng" | "manager" | "har";
export type TaskStatus = "todo" | "doing" | "review" | "done" | "paused" | "stopped";
export type Provider = "openai" | "anthropic" | "compatible" | "ollama" | "deepseek" | "siliconflow" | "zai" | "mimo" | "qwen" | "doubao";
export type SessionPermissionMode = "restricted" | "approval" | "full";

export interface ModelOption {
  id: string;
  name: string;
  provider: Provider;
  category: "chat" | "image" | "video" | "audio" | "embedding" | "rerank" | "reasoning" | "coder" | "math" | "vision" | "tools" | "fim" | "moe" | "other";
  ownedBy?: string;
  icon?: string;
}

export interface Agent {
  id: string;
  name: string;
  role: AgentRole;
  title: string;
  specialty: string;
  status: "online" | "working" | "idle";
  color: string;
  defaultModel?: string;
  sourceId?: string;
}

export interface Office {
  id: string;
  name: string;
  description: string;
  managerIds: string[];
  memberIds: string[];
  progress: number;
  goals?: string[];
  deliverables?: string[];
  plan?: string[];
  sourceId?: string;
}

export interface WorkTask {
  id: string;
  officeId: string;
  title: string;
  description: string;
  ownerId: string;
  status: TaskStatus;
  priority: "low" | "medium" | "high";
  dueDate: string;
  upstreamIds?: string[];
  downstreamIds?: string[];
  conversationId?: string;
  deliveredAt?: string;
  archived?: boolean;
  archivedAt?: string;
}

export interface AttachmentItem {
  id: string;
  name: string;
  mime: string;
  byteSize: number;
  sha256: string;
  originalPath?: string;
  cachePath: string;
  createdAt: string;
  source: "user-upload" | "agent-output";
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  content: string;
  timestamp: string;
  attachments?: AttachmentItem[];
}

export interface ConversationOptions {
  workspaceRoot: string;
  officeId?: string;
  expertId?: string;
  skillIds: string[];
  model?: string;
  permissionMode?: SessionPermissionMode;
  pendingObjective?: string;
}

export interface Conversation {
  id: string;
  title: string;
  createdAt: string;
  updatedAt: string;
  messages: ChatMessage[];
  options: ConversationOptions;
}

export interface ImSession {
  id: string;
  channel: "wechat" | "webhook" | "feishu" | "dingtalk" | "wecom" | "telegram" | "slack" | "discord" | "teams" | "matrix";
  title: string;
  externalUserId: string;
  accountId?: string;
  contextToken?: string;
  conversationId: string;
  unread: number;
  lastMessageAt: string;
}

export interface MemoryItem {
  id: string;
  title: string;
  content: string;
  createdAt: string;
  kind?: "manual" | "dream" | "summary";
  confidence?: number;
  source?: string;
  lastConfirmedAt?: string;
}

export interface SkillItem {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  category: string;
  source?: "builtin" | "openpisci" | "harmony" | "clawhub" | "skillhub" | "openai" | "local";
  sourceId?: string;
  tools?: string[];
  installPath?: string;
  version?: string;
}

export interface ScheduleItem {
  id: string;
  name: string;
  description?: string;
  cron: string;
  mode?: "cron" | "daily" | "manual";
  time?: string;
  executorId?: string;
  action?: "agent_prompt" | "dream_memory" | "custom";
  prompt?: string;
  enabled: boolean;
  lastRun?: string;
}

export interface AppSettings {
  provider: Provider;
  model: string;
  baseUrl: string;
  availableModels: ModelOption[];
  enabledModels: string[];
  workspaceRoot: string;
  language: "zh" | "en";
  theme: "light" | "dark";
  apiKeyConfigured: boolean;
  agentMaxIterations: number;
  maxOutputTokens: number;
  contextWindowTokens: number;
  toolResultTokenLimit: number;
  llmTimeoutSeconds: number;
  expertTaskTimeoutSeconds: number;
  confirmShell: boolean;
  confirmFileWrite: boolean;
  browserBackground: boolean;
  allowOutsideWorkspace: boolean;
  sessionPermissionMode?: SessionPermissionMode;
  streamOutput: boolean;
  visionEnabled: boolean;
  visionUseMainModel: boolean;
  compressionTokenThreshold: number;
  compressionMicroPercent: number;
  compressionAutoPercent: number;
  compressionFullPercent: number;
  summaryModel: string;
  dreamEnabled: boolean;
  dreamAutoRun: boolean;
  dreamIntervalHours: number;
  dreamRetentionDays: number;
  dreamMinConfidence: number;
  heartbeatEnabled: boolean;
  heartbeatIntervalMinutes: number;
  heartbeatModel: string;
  heartbeatLastRun?: string;
  memoryFileEnabled: boolean;
  memoryEmbeddingEnabled: boolean;
  memoryEmbeddingModel: string;
  objectStorageEnabled: boolean;
  objectStorageEndpoint: string;
  objectStorageUploadEndpoint: string;
  objectStorageBucket: string;
  objectStorageAccessKey: string;
  objectStorageSecretKey: string;
  objectStorageRegion: string;
  objectStorageLinkExpiryHours: number;
}

export interface AppState {
  agents: Agent[];
  offices: Office[];
  tasks: WorkTask[];
  messages: ChatMessage[];
  conversations: Conversation[];
  activeConversationId: string;
  imSessions: ImSession[];
  activeImSessionId?: string;
  memories: MemoryItem[];
  skills: SkillItem[];
  schedules: ScheduleItem[];
  settings: AppSettings;
}

export interface WorkContext {
  method: "openpiscis-compatible-work-pool";
  coordinator?: Agent;
  roster: Agent[];
  offices: Array<
    Office & {
      managers: Agent[];
      members: Agent[];
      tasks: WorkTask[];
    }
  >;
  tasks: Array<
    WorkTask & {
      owner?: Agent;
      office?: Pick<Office, "id" | "name" | "description">;
    }
  >;
  installedSkills?: SkillItem[];
  attachments?: AttachmentItem[];
  enabledSkills: SkillItem[];
  disabledSkills: Pick<SkillItem, "id" | "name" | "category">[];
  activeConversation?: Conversation;
  memories: MemoryItem[];
  schedules: ScheduleItem[];
  settings?: AppSettings;
  generatedAt: string;
}

export interface RuntimeTool {
  id: string;
  name: string;
  category: string;
  description: string;
  enabled: boolean;
}

export interface AgentRunTodo {
  id: string;
  content: string;
  status: "pending" | "in_progress" | "completed" | "cancelled";
}

export interface AgentToolStep {
  id: string;
  name: string;
  input: unknown;
  result?: string;
  isError?: boolean;
  completed: boolean;
}

export interface ChatCompletionResult {
  content: string;
  tasks: WorkTask[];
  workPoolUpdates?: WorkPoolUpdates;
  todos: AgentRunTodo[];
  toolSteps: AgentToolStep[];
}

export interface WorkPoolUpdates {
  upsertOffices?: Office[];
  deleteOfficeIds?: string[];
  upsertAgents?: Agent[];
  deleteAgentIds?: string[];
  upsertTasks?: WorkTask[];
  deleteTaskIds?: string[];
}

export type ChatStreamEvent =
  | { type: "started"; runId: string; message?: string }
  | { type: "assistant_note"; runId: string; content: string }
  | { type: "todo"; runId: string; todos: AgentRunTodo[] }
  | { type: "token"; runId: string; content: string }
  | { type: "content_reset"; runId: string; content?: string }
  | { type: "tool_start"; runId: string; step: AgentToolStep }
  | { type: "tool_end"; runId: string; step: AgentToolStep; tasks?: WorkTask[]; workPoolUpdates?: WorkPoolUpdates; todos?: AgentRunTodo[] }
  | { type: "done"; runId: string; content: string; tasks: WorkTask[]; workPoolUpdates?: WorkPoolUpdates; todos: AgentRunTodo[]; toolSteps: AgentToolStep[] }
  | { type: "stopped"; runId: string; content?: string; tasks?: WorkTask[]; workPoolUpdates?: WorkPoolUpdates; todos?: AgentRunTodo[]; toolSteps?: AgentToolStep[] }
  | { type: "error"; runId: string; error: string; tasks?: WorkTask[]; workPoolUpdates?: WorkPoolUpdates; todos?: AgentRunTodo[]; toolSteps?: AgentToolStep[] };

export interface RuntimeModule {
  id: string;
  name: string;
  description: string;
  status: "ready" | "partial" | "stub";
}

export interface RuntimeTopology {
  product: string;
  runtime: string;
  marketplaceRoot: string;
  modules: RuntimeModule[];
  tools: RuntimeTool[];
  skillMarkets: string[];
}

export interface ImChannelStatus {
  id: string;
  name: string;
  description: string;
  status: "disconnected" | "configured" | "connected" | "testing";
  capabilities: string[];
  connectionMode?: "xiaoheng-weixin-native" | "config";
  setupHint?: string;
  configSchema?: Array<{ key: string; label: string; secret?: boolean; required?: boolean; placeholder?: string }>;
  config?: Record<string, string | number | boolean>;
  updatedAt?: string;
}

export interface WeixinLoginStartResult {
  ok: boolean;
  status: "starting" | "qrcode" | "connected" | "error";
  alreadyConnected?: boolean;
  sessionKey?: string;
  qrcodeUrl?: string;
  accountId?: string;
  message?: string;
  error?: string;
}

export interface WeixinLoginPollResult {
  status: "wait" | "scaned" | "confirmed" | "connected" | "expired" | "need_verifycode" | "scaned_but_redirect" | "error";
  accountId?: string;
  message?: string;
  raw?: unknown;
}

export interface MarketItem {
  id: string;
  name: string;
  description: string;
  download_url?: string;
  source?: string;
  trusted?: boolean;
  tags?: string[];
  featured?: boolean;
  slug?: string;
  version?: string;
  downloads?: number;
  stars?: number;
  installPath?: string;
}

export interface MarketplaceAggregate {
  source: string;
  experts: MarketItem[];
  teams: MarketItem[];
  skills: MarketItem[];
  skillMarkets: string[];
}

export interface InstallPayload {
  agents?: Agent[];
  offices?: Office[];
  skills?: SkillItem[];
  message: string;
}
