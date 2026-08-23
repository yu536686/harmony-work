# 小亨 WorkBuddy Codex Harness 全面迁移可行性报告

> 日期：2026-08-23  
> 目标版本线：b1.0.x  
> 首个工程版本：`1.0.0-beta.1`（产品展示为 `b1.0.0`）  
> 代码基线：`beabc15` / WorkBuddy 0.9.5  
> Codex 基线：`@openai/codex@0.149.0`

## 1. 结论

项目具备迁移条件，推荐采用 **Codex app-server 作为交互式 Agent 运行时、Codex exec 作为无人值守运行时、Codex SDK 作为服务端和自动化集成层**。WorkBuddy 不应 fork 或直接嵌入 Codex Core，也不应让三个入口同时处理同一类会话。

该迁移属于底层运行时替换，不是 UI 重写。现有 React/Tauri UI、Harvis、StaffDeck、素材库、营销工作台、登录、IM、技能市场和文件成果模块可以保留；需要重写的是会话执行适配层、工具宿主边界、审批映射、线程持久化和配置生成。

结论为 **有条件可行**：

- 首期模型必须提供 OpenAI Responses API。当前 Codex 主线已经移除 Chat Completions wire API。
- DeepSeek 官方 Responses API 可以作为首期非 OpenAI 模型，支持 function、Codex `apply_patch` custom tool 和流式 SSE。
- WorkBuddy 专用工具必须迁移为 MCP 或 app-server dynamic tool，不能继续依赖 `piscis_kernel::Tool`。
- app-server 的 WebSocket 传输仍标记为实验性，桌面端必须使用 stdio JSONL。
- dynamic tools 仍是实验接口，生产工具以 MCP 为最终边界，dynamic tools 只用于迁移期或必须由桌面 UI 同步响应的能力。

## 2. 官方能力依据

- [`codex app-server`](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md) 是 Codex 富客户端的官方接口，使用双向 JSON-RPC，支持线程、轮次、增量事件、审批、技能、MCP、账户和配置。
- [`@openai/codex-sdk`](https://github.com/openai/codex/blob/main/sdk/typescript/README.md) 通过启动 Codex CLI 并交换 JSONL 事件提供线程、流式结果、结构化输出和恢复能力。
- [Codex model provider 源码](https://github.com/openai/codex/blob/main/codex-rs/model-provider-info/src/lib.rs) 当前只保留 Responses wire API，`wire_api = "chat"` 会直接报错。
- [DeepSeek Responses API](https://api-docs.deepseek.com/zh-cn/guides/responses_api/) 提供 `https://api.deepseek.com` 基地址，支持 `deepseek-v4-flash`、`deepseek-v4-pro`、工具调用和 Codex `apply_patch`。

## 3. 当前系统耦合评估

0.9.5 基线中，Piscis 不是一个可单文件替换的库：

- `src-tauri/Cargo.toml` 直接依赖 `piscis-core`、`piscis-kernel`、`piscis-ide-tools`。
- 会话主入口 `src-tauri/src/commands/chat.rs` 同时承担模型配置、上下文组装、历史压缩、计划、工具注册、Agent Loop、事件转发、持久化和后台记忆整理。
- `AppState` 直接复用 Piscis 的 `Database`、`Settings`、计划状态、调度器和确认通道。
- RobotZ、Office Artifact、Harvis、IM 和 StaffDeck 工具实现 `piscis_kernel::Tool` 或依赖其结果类型。
- 当前源码与 `third_party` 中约 196 个文件包含 Piscis 引用；其中多数位于待删除的内置引擎，但桌面宿主也存在直接耦合。

因此不能先删除 `third_party/piscis-engine` 再补功能。正确顺序是先建立 Codex Host Adapter，使每条现有业务路径切换到新接口，最后移除 Piscis crate 和旧数据库实现。

## 4. 推荐目标架构

```text
React UI
  | Tauri IPC
WorkBuddy Application Host
  |-- Session Index / User / Project / Artifacts
  |-- Codex AppServer Supervisor ---- stdio JSON-RPC ---- codex app-server
  |-- Approval + Event Adapter                   |
  |-- WorkBuddy MCP Host <-----------------------+
  |     |-- Harvis / Office Artifact / IM / Skills
  |     |-- StaffDeck / Marketing / Memory / RobotZ
  |
  |-- Headless Runner ---- codex exec --json
  |-- Server Automation -- @openai/codex-sdk
```

职责边界：

| 组件 | 唯一职责 |
|---|---|
| app-server | 交互式线程、Agent Loop、上下文、计划、工具编排、审批和执行事件 |
| codex exec | IM、CLI、定时任务和后台一次性执行 |
| Codex SDK | 8090 服务端、回归测试和外部工作流集成 |
| WorkBuddy Host | UI IPC、登录、项目、业务数据、进程监管、事件转换和成果索引 |
| WorkBuddy MCP Host | 将现有独立功能暴露为标准工具，不包含 Agent Loop |

## 5. 模型与配置策略

### OpenAI

使用 Codex 原生认证或 `OPENAI_API_KEY`，不复制凭据到会话数据库。

### DeepSeek

WorkBuddy 为每个登录用户生成独立的 `CODEX_HOME`，写入以下配置骨架，密钥仅通过环境变量注入：

```toml
model = "deepseek-v4-pro"
model_provider = "deepseek"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
env_key = "DEEPSEEK_API_KEY"
wire_api = "responses"
supports_websockets = false
```

DeepSeek Responses API 是无状态接口，不支持 `previous_response_id` 和 `conversation`。这不阻塞迁移，因为 Codex app-server 持有并持久化完整线程历史，但要求 WorkBuddy 不自行裁剪后再伪造 provider 会话。

### 模型切换

UI 模型选择必须转换为 `turn/start.model` 和 provider profile。WorkBuddy 不再自行构造模型 HTTP 请求，也不再维护第二套 Agent 上下文。非 Responses provider 在 b1.0 首期显示为“不兼容 Codex Harness”，不可选而不是静默回退。

## 6. 会话与数据迁移

迁移期保留 WorkBuddy 会话 ID 作为 UI 和业务主键，增加映射：

```text
workbuddy_session_id -> codex_thread_id
user_id              -> isolated CODEX_HOME
workspace_root       -> thread cwd / runtime roots
```

Codex rollout 是 Agent 历史的权威来源；WorkBuddy 数据库只保存：

- 会话标题、项目、来源渠道和登录用户。
- Codex thread ID、当前 turn ID、运行状态和最后同步时间。
- 业务 Artifact、IM binding、StaffDeck task 和营销数据引用。
- 供 UI 快速加载的消息投影，不再反向注入模型上下文。

旧 `piscis.db` 通过一次性迁移复制到 `workbuddy.db`。迁移完成前保持只读备份，不做原地改名或破坏性升级。

## 7. 工具迁移

### 保留 Codex 原生工具

- shell / unified exec
- apply_patch
- 文件读取、搜索和变更
- web search（模型和配置支持时）
- plan、request user input、权限请求
- MCP、skills、subagents

### 迁移到 WorkBuddy MCP Host

- Harvis 图片和音乐
- Office Artifact 文档、演示和表格
- IM 查询、回复和文件发送
- StaffDeck 人员与任务分派
- 素材库、营销数据看板
- 长期记忆和技能市场
- RobotZ 浏览器、桌面和 Windows UIA

MCP 工具返回结构化 content、resource link 和 Artifact metadata。WorkBuddy 从 app-server 的工具 item 中提取成果并写入现有 Artifacts 面板。

### 暂时使用 dynamic tools

`chat_ui`、审批窗口等需要当前桌面窗口同步交互的能力可以先使用 `dynamicTools`。因为它属于实验 API，必须：

- 固定 Codex 版本。
- 在构建时生成并提交 app-server TypeScript/JSON Schema。
- 版本升级先跑协议契约测试。
- 为每项 dynamic tool 准备 MCP 迁移路径。

## 8. UI 事件映射

| app-server 事件 | WorkBuddy UI |
|---|---|
| `turn/started` | 会话进入运行中 |
| `item/agentMessage/delta` | 助手消息流式文本 |
| `item/started` / `item/completed` | Activity / Tools |
| `turn/plan/updated` | Todo |
| `turn/diff/updated` | 文件变更和撤销入口 |
| command/file approval request | 当前会话内快速批准弹窗 |
| `thread/tokenUsage/updated` | 上下文与 Token 指示器 |
| `turn/completed` | 完成、失败或中断状态 |

前端不展示原始思维链。Reasoning 只显示 Codex 提供的摘要，并允许用户折叠。

## 9. 风险

| 风险 | 等级 | 控制措施 |
|---|---:|---|
| Codex app-server 协议快速变化 | 高 | 固定 0.149.0、生成 schema、契约测试、升级清单 |
| DeepSeek Responses 部分字段忽略 | 中 | 禁用依赖 `store`、`previous_response_id`、service tier 的路径 |
| dynamic tools 为实验 API | 高 | 核心业务使用 MCP；dynamic tools 只做桌面交互桥接 |
| Windows sidecar 打包和 Defender | 高 | 固定官方平台包、哈希校验、代码签名、禁止运行时下载 |
| 双数据库导致消息重复 | 中 | Codex rollout 为权威历史，WorkBuddy 只保存投影和业务索引 |
| IM 和定时任务线程恢复 | 中 | 建立 binding-thread 表，codex exec 使用固定 CODEX_HOME 和恢复 ID |
| Piscis 移除破坏 StaffDeck | 高 | 先把人员/任务数据提取为独立 domain crate，再删除 pool/koi 代码 |

## 10. 交付阶段

1. **b1.0.0-dev.1：** app-server supervisor、协议生成、健康检查、DeepSeek 配置和 shadow thread。
2. **b1.0.0-dev.2：** 新会话使用 app-server，完成文本、附件、取消、审批和事件映射。
3. **b1.0.0-dev.3：** WorkBuddy MCP Host 接管 Harvis、Office、IM、Skill、Memory 和 RobotZ。
4. **b1.0.0-dev.4：** codex exec 接管 IM/定时/CLI；SDK 接入服务端自动化。
5. **b1.0.0-beta.1：** 数据迁移、删除 Piscis Agent Loop 与 LLM clients，Windows/macOS 打包和回归。

每一阶段都必须能回滚到上一个内部构建。最终 beta 安装包不再包含 Piscis 运行时；迁移期分支允许保留旧代码作为对照，但默认不会执行。

## 11. 验收指标

- 简单问候不创建计划、不调用工具，只有一个模型请求。
- 单条普通会话不存在 WorkBuddy 侧二次意图路由和二次上下文组装。
- Activity 中每个执行 item 均有 started/completed 闭环。
- 请求批准、会话级批准、拒绝和取消都可恢复当前 turn。
- Harvis、Office、素材库、IM、StaffDeck、技能市场和长期记忆回归测试全部通过。
- 新建、恢复、分叉、删除和项目切换不丢失 Codex thread 映射。
- Windows 和 macOS 安装包离线包含匹配平台的 Codex binary，不在首次运行时联网安装运行时。
- 源码和 Cargo 依赖中不再包含 `piscis-core`、`piscis-kernel`、`piscis-ide-tools`。

