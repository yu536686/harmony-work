# Codex Harness Phase 1 验证记录

## 版本与分支

- 分支：`codex/b1.0-codex-app-server`
- 应用版本：`1.0.0-beta.1`
- 展示版本：`b1.0.0`
- 固定 Codex 版本：`0.149.0`
- 模型协议：仅 Responses API

## 已完成的运行通道

### 桌面交互：app-server

- 使用 `codex app-server --listen stdio://` 常驻子进程。
- 完成 initialize/initialized 握手、请求关联、通知和服务端审批分发。
- WorkBuddy 会话 ID 与 Codex thread ID 独立持久化。
- thread/start、thread/resume、turn/start、turn/cancel 和审批响应已接通。
- app-server 事件适配到现有活动、待办、工具、权限和 token UI 事件契约。

### IM、定时任务和无头执行：codex exec

- 使用 `codex exec --json`，不通过 Shell 拼接用户输入。
- 支持工作区、模型、权限、图片附件和普通文件路径上下文。
- 解析 thread、最终回复、token usage 和原始 JSONL 事件。

### 服务端自动化：Codex SDK

- 固定依赖 `@openai/codex-sdk@0.149.0`。
- `scripts/codex-sdk-runner.mjs` 从 stdin 接收结构化任务并输出 JSONL。
- 支持 start/resume thread、三种权限、图片和普通文件附件。
- DeepSeek 通过自定义 provider 使用 `wire_api = "responses"`。
- API 密钥仅由 provider 的 `env_key` 从子进程环境读取。

## 发布运行时

- `npm run stage:codex` 从固定 npm 平台包暂存原生 Codex 二进制。
- Tauri 将 `resources/codex` 打入安装包。
- 运行时按显式路径、安装包资源、项目固定 npm 包、PATH 的顺序解析。
- app-server 启动前执行 `codex --version`，版本必须严格等于 `0.149.0`。
- 构建过程不联网下载 Codex；缺少平台包时直接失败。

## 验证结果

2026-08-23 在 macOS arm64 工作树执行：

- Codex Runtime Rust 测试：30/30 通过。
- Rust 全量 lib 测试：124 通过，1 个真实网络测试忽略，0 失败。
- 前端测试：39/39 通过。
- Artifact Runtime：5/5 通过。
- Codex SDK runner：4/4 通过。
- Codex sidecar staging：3/3 通过，实际暂存版本 `0.149.0`。
- Web 生产构建：通过；保留已有动态导入和大 chunk 警告。
- macOS `.app` release 构建：通过，路径为
  `target/release/bundle/macos/小亨 WorkBuddy.app`。
- 应用包内 sidecar：`Contents/Resources/resources/codex/codex`，
  `--version` 返回 `codex-cli 0.149.0`，initialize/initialized 真实握手通过。
- 当前 macOS 包体约 242 MB，其中 Codex 原生二进制约 210 MB。
- 新增 Codex Rust 文件独立 `rustfmt --check`：通过。

全仓 `cargo fmt --check` 仍会报告基线中未由本分支修改的
`marketing.rs`、Harvis 和 Office Artifact 文件格式差异。本分支未借机改写这些无关文件。

## 当前边界

本阶段提供并行的 Codex 纵向链路，但没有把已有 `chat_send` 默认改到 Codex。
这是为了在 WorkBuddy MCP 工具宿主、Harvis、StaffDeck、IM 消息持久化和历史会话迁移完成前，
不破坏 0.9.5 的业务功能。

尚未完成：

- 把 WorkBuddy 内置业务工具迁移为 Codex 可发现的 MCP 工具。
- 把旧消息数据库与 Codex thread 的消息落库统一。
- 设置变更后热更新 app-server provider 凭据。
- 使用真实 DeepSeek 密钥完成 Responses function-call 回环测试。
- 将默认桌面对话和 IM/定时任务分别切换到 app-server 与 exec/SDK。
- 完成迁移后删除 Piscis runtime、旧 loop 和重复工具路由。

因此，`b1.0.0` 是 Codex Harness 迁移的可运行基础版本，不是最终去 Piscis 版本。
