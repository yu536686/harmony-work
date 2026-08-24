# WorkBuddy b1.0 Codex Harness 迁移设计

> 状态：已批准  
> 基线：WorkBuddy 0.9.5 (`beabc15`)  
> 目标：`1.0.0-beta.1` / 展示版本 `b1.0.0`

## 目标

以 Codex app-server 完整替换 Piscis 交互式 Agent Loop，以 Codex exec 替换 headless/IM/定时执行入口，以 Codex SDK 提供外部自动化入口，同时保持现有 UI 和独立业务模块的行为与数据。

## 架构决策

### 采用 sidecar，不 fork Codex

Tauri 启动固定版本的官方 `codex` 平台二进制，通过 stdio JSONL 与 `codex app-server` 通信。WorkBuddy 不链接 Codex 内部 Rust crates，避免上游私有类型和内部重构扩散到应用代码。

### 单一会话权威

app-server thread 是 Agent 上下文权威，WorkBuddy session 是业务/UI 权威。二者通过持久映射关联，WorkBuddy 不再把自己的消息表重新拼装后发送给模型。

### 工具采用 MCP-first

独立业务能力由 `workbuddy-mcp-host` 暴露。Codex 原生文件、shell、patch、plan 和审批工具不重复实现。仅当前窗口专属交互在迁移期使用 dynamic tools。

### 独立 CODEX_HOME

每个 WorkBuddy 登录用户使用应用数据目录下的独立 Codex Home，避免读取用户个人 Codex 会话、插件、认证和不兼容配置。

## 组件

### `CodexAppServerSupervisor`

- 解析 bundled/system Codex binary。
- 启动 `codex app-server --listen stdio://`。
- 完成 initialize/initialized 握手。
- 维护请求 ID、pending responses、通知广播、server request 回包和异常重启。
- 对 stderr 做结构化日志采集，但不得混入 JSONL stdout。

### `CodexSessionBridge`

- 创建、恢复和中断 Codex thread/turn。
- 将 WorkBuddy model、workspace、attachments、skills 和 permission profile 转为 app-server 参数。
- 管理 `workbuddy_session_id -> codex_thread_id`。
- 只允许同一 WorkBuddy session 存在一个 active turn。

### `CodexEventAdapter`

- 将 app-server 通知映射为现有 `AgentEventType`，使前端第一阶段无需改布局。
- 保留原始 thread/turn/item ID，禁止用数组下标作为 UI 主键。
- `item/completed` 是工具结果的权威状态。

### `WorkBuddyMcpHost`

- 以 stdio MCP server 方式运行。
- 调用现有 Harvis、Office、IM、Skill、Memory、Marketing、StaffDeck 和 RobotZ domain service。
- 不直接访问 React/Tauri window；需要 UI 输入时使用 MCP elicitation 或 dynamic tool bridge。

### `CodexHeadlessRunner`

- 使用 `codex exec --json` 处理 IM、CLI、scheduler 和后台整理任务。
- 使用稳定绑定恢复 thread，不为一次任务重复创建计划或额外 router 调用。
- 事件写入同一 WorkBuddy activity/artifact 投影。

### `CodexSdkGateway`

- 部署在 Node.js 服务端或测试工具，不进入桌面会话热路径。
- 为 8090 服务端和批量自动化提供 `startThread`、`resumeThread`、`runStreamed`。

## 数据流

```text
User -> React -> chat_send
  -> SessionBridge.turn_start
  -> app-server -> Responses provider
  -> command/MCP/dynamic tool
  -> item events -> EventAdapter -> existing Activity/Todo/Tools/Artifacts
  -> turn/completed -> message projection + session status
```

## 错误处理

- app-server 未启动：阻止发送，展示运行时诊断和修复按钮。
- stdout 非法 JSON：记录原始行和 Codex 版本，重启 sidecar，不把协议文本显示成助手消息。
- `Server overloaded`：带 jitter 指数退避，最多三次，不重复创建 turn。
- provider SSE 断开：按 Codex 的 terminal error 分类展示，不做 WorkBuddy 自定义无工具重试。
- tool request 无响应：绑定 turn cancellation；用户取消后必须清除弹窗。
- sidecar 崩溃：未完成 turn 标记 interrupted，进程恢复后显式 resume thread，不假装任务完成。

## 安全

- API key 仅通过 child process environment 注入。
- 默认 permission profile 映射：受限=`:read-only`，请求批准=`:workspace`+user approvals，完全访问=`:full-access`。
- WorkBuddy MCP Host 每个调用校验用户、session、workspace roots 和工具风险。
- bundled Codex binary 记录版本和 SHA-256；安装包签名覆盖 sidecar。
- 不启用 app-server WebSocket listener。

## 迁移兼容

开发里程碑允许 `agent_runtime = piscis | codex` 作为内部诊断开关；公开 b1.0 beta 只允许 codex。旧数据库只读保留一个版本周期，回滚由安装器恢复副本，不在运行时双写 Agent 历史。

## 测试

- 协议单元测试：请求关联、乱序通知、非法 JSON、server request。
- 进程集成测试：fake app-server JSONL fixture。
- 契约测试：由 pinned Codex 生成 schema 后校验本地类型。
- UI 回归：现有 39 项测试保持通过并增加 app-server event fixtures。
- 业务回归：Harvis、Office、IM、StaffDeck、素材库、技能市场和记忆。
- 安装测试：Windows x64、macOS arm64，完全离线启动 bundled Codex。

## 非目标

- b1.0 首期不兼容 Chat Completions-only provider。
- 不复制 Codex Desktop UI。
- 不展示模型原始思维链。
- 不使用实验 WebSocket 作为本地传输。
- 不在 WorkBuddy 中维护第二套 Agent Loop。

