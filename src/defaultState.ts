import type { AppState } from "./types";

export const defaultState: AppState = {
  agents: [
    {
      id: "xiaoheng",
      name: "小亨",
      role: "xiaoheng",
      title: "主协调 Agent",
      specialty: "负责自主理解用户目标，拆解 todo，选择项目组、成员和技能，驱动任务从计划、执行、待验收到已交付；关键步骤后必须复核，遇到错误、缺授权或缺信息时标记暂停并说明恢复条件。",
      status: "online",
      color: "#2563eb"
    },
    {
      id: "manager-product",
      name: "产品经理",
      role: "manager",
      title: "产品经理",
      specialty: "你是产品经理。负责把用户目标转化为清晰需求、验收标准、优先级和里程碑；在执行中持续检查范围是否失控，识别用户真正要交付的结果，并在小亨验收前给出产品侧复核意见。",
      status: "working",
      color: "#0891b2"
    },
    {
      id: "manager-tech",
      name: "技术经理",
      role: "manager",
      title: "技术经理",
      specialty: "你是技术经理。负责技术方案、工具调用路径、代码与脚本执行、失败恢复、权限边界和质量把关；遇到报错要定位根因，给出可执行修复方案，并复核成员提交的技术交付物。",
      status: "online",
      color: "#7c3aed"
    },
    {
      id: "har-designer",
      name: "设计师",
      role: "har",
      title: "设计师",
      specialty: "你是设计师。负责界面信息架构、视觉层级、交互状态、文案呈现和可用性检查；产出布局建议、组件规范、视觉优化方案，并确保复杂工作流页面清晰高效。",
      status: "online",
      color: "#db2777"
    },
    {
      id: "har-dev",
      name: "开发专员",
      role: "har",
      title: "开发职员",
      specialty: "你是开发专员。负责具体执行编码、脚本、文件读写、数据处理、API 调用、打包构建和验证；完成后提交可检查产物路径、关键日志、测试结果和复核要点给小亨验收。",
      status: "working",
      color: "#0f766e"
    },
    {
      id: "har-research",
      name: "研究专员",
      role: "har",
      title: "研究职员",
      specialty: "你是研究专员。负责网页检索、公开资料采集、新闻抓取、信息去重、结构化摘要和来源整理；遇到登录、反爬或网络失败时，提交阻塞原因、替代来源和可复核证据。",
      status: "idle",
      color: "#d97706"
    },
    {
      id: "har-market",
      name: "市场专家",
      role: "har",
      title: "市场专家",
      specialty: "你是市场专家。负责竞品分析、用户画像、渠道策略、卖点提炼、增长假设和投放复盘；输出要包含目标人群、市场机会、风险判断和可执行的下一步动作。",
      status: "online",
      color: "#ea580c"
    },
    {
      id: "har-content-manager",
      name: "内容经理",
      role: "manager",
      title: "内容经理",
      specialty: "你是内容经理。负责内容策略、选题规划、账号定位、发布节奏、质量标准和效果复盘；协调内容专员与编导，把用户目标拆成可执行的内容生产计划。",
      status: "online",
      color: "#4f46e5"
    },
    {
      id: "har-content",
      name: "内容专员",
      role: "har",
      title: "内容专员",
      specialty: "你是内容专员。负责资料整理、标题提案、正文撰写、脚本初稿、标签关键词、平台适配和发布检查；交付内容要贴合目标受众并便于复核修改。",
      status: "idle",
      color: "#16a34a"
    },
    {
      id: "har-director",
      name: "编导",
      role: "har",
      title: "编导",
      specialty: "你是编导。负责视频/图文叙事结构、分镜、节奏、镜头脚本、拍摄清单和后期说明；需要把抽象需求转成可拍、可剪、可交付的执行方案。",
      status: "idle",
      color: "#9333ea"
    }
  ],
  offices: [
    {
      id: "office-market-research",
      name: "市场调研小组",
      description: "负责行业资料、竞品动态、用户需求、渠道趋势和公开信息整理。",
      managerIds: ["manager-product"],
      memberIds: ["har-research", "har-market"],
      progress: 0,
      goals: ["快速收集并验证外部信息", "形成可复核的结构化调研结论"],
      deliverables: ["调研摘要", "竞品/渠道对比表", "来源清单"],
      plan: ["明确调研目标", "采集公开资料", "结构化整理", "提交结论和建议"]
    },
    {
      id: "office-online-marketing",
      name: "线上营销小组",
      description: "负责线上活动、投放方案、增长实验、渠道运营和效果复盘。",
      managerIds: ["har-content-manager"],
      memberIds: ["har-market", "har-content", "har-director"],
      progress: 0,
      goals: ["把营销目标拆成可执行动作", "持续跟踪效果并优化策略"],
      deliverables: ["营销方案", "投放/发布计划", "复盘报告"],
      plan: ["确定目标人群", "设计渠道策略", "安排执行节奏", "复盘数据表现"]
    },
    {
      id: "office-content",
      name: "内容小组",
      description: "负责选题、脚本、图文/视频内容、平台适配和发布检查。",
      managerIds: ["har-content-manager"],
      memberIds: ["har-content", "har-director", "har-designer"],
      progress: 0,
      goals: ["稳定产出符合平台和品牌要求的内容", "让内容从选题到交付可追踪"],
      deliverables: ["选题表", "内容稿件", "拍摄/剪辑脚本", "发布检查清单"],
      plan: ["确定账号定位", "规划选题", "撰写/编导脚本", "检查并交付"]
    }
  ],
  tasks: [
  ],
  messages: [
    {
      id: "welcome",
      role: "assistant",
      content:
        "你好，我是小亨。现在我可以读取团队、项目、任务、技能和运行时工具。你可以直接让我抓取资料、整理表格、安排团队或从市场安装专家和技能。",
      timestamp: new Date().toISOString()
    }
  ],
  conversations: [
    {
      id: "conversation-default",
      title: "你好，我是小亨",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      messages: [
        {
          id: "welcome",
          role: "assistant",
          content:
            "你好，我是小亨。现在我可以读取团队、项目、任务、技能和运行时工具。你可以直接让我抓取资料、整理表格、安排团队或从市场安装专家和技能。",
          timestamp: new Date().toISOString()
        }
      ],
      options: {
        workspaceRoot: "",
        officeId: "",
        expertId: "xiaoheng",
        skillIds: ["skill-skill-search", "skill-news-xlsx", "skill-web-fetch", "skill-file-write", "skill-shell-exec", "skill-desktop-control"],
        model: "gpt-4.1-mini"
      }
    }
  ],
  activeConversationId: "conversation-default",
  imSessions: [],
  activeImSessionId: undefined,
  memories: [
    {
      id: "memory-openpiscis",
      title: "运行时方向",
      content: "后端按 OpenPiscis 的初始化、团队、技能、市场、IM 渠道和工具注册方式组织。",
      createdAt: "2026-06-22",
      kind: "summary",
      confidence: 90,
      source: "system"
    }
  ],
  skills: [
    {
      id: "skill-skill-search",
      name: "技能检索",
      description: "在已安装技能中检索可用能力，供 Agent 决策前调用。",
      enabled: true,
      category: "运行时",
      source: "builtin",
      tools: ["skill_search", "skill_list"]
    },
    {
      id: "skill-news-xlsx",
      name: "新闻头条 Excel",
      description: "抓取百度、腾讯、新浪、网易等新闻头条，生成真实 .xlsx 文件并保存到当前会话工作空间。",
      enabled: true,
      category: "办公自动化",
      source: "builtin",
      tools: ["news_to_excel"]
    },
    {
      id: "skill-web-fetch",
      name: "网页读取",
      description: "通过后端网络请求读取公开网页文本，供 Agent 整理分析。",
      enabled: true,
      category: "研究",
      source: "builtin",
      tools: ["web_fetch"]
    },
    {
      id: "skill-file-write",
      name: "本地文件写入",
      description: "读写当前会话工作空间中的文本文件，并列出目录内容。",
      enabled: true,
      category: "文件",
      source: "builtin",
      tools: ["write_text_file", "read_text_file", "list_files"]
    },
    {
      id: "skill-shell-exec",
      name: "Shell 命令执行",
      description: "在当前会话工作空间执行 PowerShell 或 CMD 命令，用于脚本、构建、转换和本地自动化。",
      enabled: true,
      category: "系统",
      source: "builtin",
      tools: ["shell_exec"]
    },
    {
      id: "skill-desktop-control",
      name: "桌面与浏览器控制",
      description: "为浏览器控制、模拟点击和桌面自动化保留统一工具接口；运行时不可用时会让任务进入暂停并说明缺少的能力。",
      enabled: true,
      category: "桌面自动化",
      source: "builtin",
      tools: ["browser_control", "computer_control"]
    },
    {
      id: "skill-memory-md",
      name: "长期记忆 MEMORY.md",
      description: "以 MEMORY.md 文件保存长期记忆，并提供低成本检索、读取和追加能力。",
      enabled: true,
      category: "记忆",
      source: "builtin",
      tools: ["memory_search", "memory_read", "memory_append"]
    }
  ],
  schedules: [
    {
      id: "automation-dream-memory-0400",
      name: "每天 4 点整理归纳记忆",
      description: "小亨在每天 04:00 汇总近期对话、任务、项目进度和用户偏好，沉淀为长期记忆。",
      cron: "0 4 * * *",
      mode: "daily",
      time: "04:00",
      executorId: "xiaoheng",
      action: "dream_memory",
      prompt: "整理最近对话、任务、项目和用户偏好，形成可复用的长期记忆；只保存长期有效事实、决策、偏好和未完成承诺。",
      enabled: true
    }
  ],
  settings: {
    provider: "openai",
    model: "gpt-4.1-mini",
    baseUrl: "https://api.openai.com/v1",
    availableModels: [{ id: "gpt-4.1-mini", name: "gpt-4.1-mini", provider: "openai", category: "chat", ownedBy: "OpenAI", icon: "◎" }],
    enabledModels: ["gpt-4.1-mini"],
    workspaceRoot: "",
    language: "zh",
    theme: "light",
    apiKeyConfigured: false,
    agentMaxIterations: 24,
    maxOutputTokens: 8192,
    contextWindowTokens: 125000,
    toolResultTokenLimit: 3000,
    llmTimeoutSeconds: 120,
    expertTaskTimeoutSeconds: 600,
    confirmShell: true,
    confirmFileWrite: true,
    browserBackground: true,
    allowOutsideWorkspace: false,
    streamOutput: true,
    visionEnabled: false,
    visionUseMainModel: true,
    compressionTokenThreshold: 200000,
    compressionMicroPercent: 60,
    compressionAutoPercent: 80,
    compressionFullPercent: 95,
    summaryModel: "",
    dreamEnabled: true,
    dreamAutoRun: true,
    dreamIntervalHours: 4,
    dreamRetentionDays: 30,
    dreamMinConfidence: 80,
    heartbeatEnabled: true,
    heartbeatIntervalMinutes: 60,
    heartbeatModel: "",
    heartbeatLastRun: "",
    memoryFileEnabled: true,
    memoryEmbeddingEnabled: false,
    memoryEmbeddingModel: "",
    objectStorageEnabled: true,
    objectStorageEndpoint: "http://61.150.31.150:8788",
    objectStorageUploadEndpoint: "http://192.168.11.29:8788",
    objectStorageBucket: "workbuddy",
    objectStorageAccessKey: "workbuddy-client",
    objectStorageSecretKey: "CN74np5NxcY7LlEYRGGdXvUElTBfF5192qRMzI52",
    objectStorageRegion: "us-east-1",
    objectStorageLinkExpiryHours: 24
  }
};
