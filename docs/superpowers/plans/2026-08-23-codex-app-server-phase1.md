# Codex app-server 基础运行时实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 在不改变默认 0.9.5 会话行为的前提下，建立可测试的 Codex app-server sidecar、协议客户端、DeepSeek Responses 配置和 shadow thread 纵向链路。

**架构：** 新增独立 `codex_runtime` 模块管理 stdio JSON-RPC 和线程映射；现有 `chat_send` 暂时保留，通过内部 runtime switch 启用 shadow 模式。第一阶段不迁移业务工具，也不删除 Piscis，完成后再进入正式会话切换阶段。

**技术栈：** Rust、Tokio、Serde、Tauri 2、Codex app-server 0.149.0、SQLite、Vitest。

---

## 文件结构

- 创建 `src-tauri/src/codex_runtime/mod.rs`：模块公开边界。
- 创建 `src-tauri/src/codex_runtime/protocol.rs`：最小 JSON-RPC、initialize、thread 和 turn 类型。
- 创建 `src-tauri/src/codex_runtime/client.rs`：请求关联、通知和 server request 分发。
- 创建 `src-tauri/src/codex_runtime/process.rs`：Codex binary 解析与 child 生命周期。
- 创建 `src-tauri/src/codex_runtime/config.rs`：独立 CODEX_HOME 和 DeepSeek Responses 配置。
- 创建 `src-tauri/src/codex_runtime/event_adapter.rs`：app-server 事件到 WorkBuddy 事件的纯函数映射。
- 创建 `src-tauri/src/codex_runtime/session_map.rs`：WorkBuddy session 与 Codex thread 映射。
- 修改 `src-tauri/src/lib.rs`：注册 runtime state 和诊断命令。
- 修改 `src-tauri/src/store/mod.rs`：挂载 supervisor，不改变现有 DB 类型。
- 修改 `src-tauri/src/commands/chat.rs`：增加内部 shadow 开关，不改变默认执行路径。
- 修改 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`：版本更新到 `1.0.0-beta.1`。

### 任务 1：版本和运行时配置模型

- [ ] 编写失败测试，断言展示版本为 `b1.0.0`、内部版本为 `1.0.0-beta.1`，DeepSeek provider 固定 `wire_api = responses` 且不把 API key 写入 TOML。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml codex_runtime::config::tests -- --nocapture`，确认因模块不存在失败。
- [ ] 实现 `CodexRuntimeConfig`、`DeepSeekProfile` 和配置渲染函数。
- [ ] 重跑目标测试和前端版本测试，确认通过。
- [ ] 提交 `feat(codex): establish b1 runtime configuration`。

### 任务 2：最小 JSON-RPC 协议

- [ ] 编写失败测试覆盖 response、notification、server request 和非法消息四种输入。
- [ ] 实现带泛型 payload 的 `JsonRpcEnvelope`、请求 ID 和 error 类型。
- [ ] 验证未知 notification 可保留原始 JSON 而不会中断读取循环。
- [ ] 运行目标测试，确认通过。
- [ ] 提交 `feat(codex): add app-server protocol foundation`。

### 任务 3：app-server client

- [ ] 使用 `tokio::io::duplex` 编写失败测试：两个并发请求响应乱序时仍按 ID 返回正确调用者。
- [ ] 编写失败测试：server request 会进入独立 channel，notification 不占用 pending request。
- [ ] 实现 reader task、writer mutex、pending map 和 shutdown。
- [ ] 验证 EOF 会令所有 pending request 返回 `AppServerClosed`。
- [ ] 提交 `feat(codex): implement jsonl app-server client`。

### 任务 4：进程监管和初始化握手

- [ ] 编写 fake app-server fixture，验证 argv 为 `app-server --listen stdio://`，stderr 与 stdout 分离。
- [ ] 实现 bundled path、设置 path 和 PATH 三层解析，禁止静默下载。
- [ ] 实现 initialize 后发送 initialized notification；握手失败终止 child。
- [ ] 增加诊断结果：binary path、version、process state、last error。
- [ ] 提交 `feat(codex): supervise pinned app-server process`。

### 任务 5：事件适配

- [ ] 编写 fixture 测试覆盖 agent delta、command start/end、plan update、approval、token usage、turn complete/error。
- [ ] 实现纯函数 `map_app_server_event`，保留 thread/turn/item ID。
- [ ] 对 reasoning 只映射 summary，不映射 raw content。
- [ ] 验证 `item/completed` 覆盖此前 started 状态。
- [ ] 提交 `feat(codex): map app-server lifecycle into WorkBuddy activity`。

### 任务 6：会话映射

- [ ] 编写数据库迁移测试，为已有 session 新增独立 mapping 表且不修改历史消息。
- [ ] 实现 create/get/delete mapping 和 active turn 状态。
- [ ] 验证删除 WorkBuddy session 会删除映射但不直接删除 Codex rollout，由清理任务处理。
- [ ] 提交 `feat(codex): persist WorkBuddy thread mappings`。

### 任务 7：shadow thread 纵向链路

- [ ] 编写 fake app-server 集成测试：创建 WorkBuddy session、start thread、start turn、接收 delta、完成并保存映射。
- [ ] 在仅内部环境变量 `WORKBUDDY_CODEX_SHADOW=1` 时启动 shadow；默认路径保持 Piscis。
- [ ] shadow 只记录协议和事件，不向模型重复发送真实用户内容；使用固定本地 fixture prompt 验证链路，避免双重计费。
- [ ] 暴露 Tauri 诊断命令，不在用户普通 UI 中增加临时入口。
- [ ] 提交 `feat(codex): add non-invasive shadow runtime probe`。

### 任务 8：完整验证

- [ ] 运行 `npm test -- --run`，预期全部通过。
- [ ] 运行 `npm run test:artifact-runtime -- --run`，预期全部通过。
- [ ] 运行 `cargo test --manifest-path src-tauri/Cargo.toml`，预期除真实网络测试外全部通过。
- [ ] 运行 `npm run build:web`，预期构建成功。
- [ ] 扫描 `rg -n "TODO|待定|wire_api = \\"chat\\"" docs src-tauri/src/codex_runtime`，预期无占位符和 Chat wire 配置。
- [ ] 提交 `docs(codex): record phase one verification`。

## 第二阶段入口条件

只有以下条件全部满足才允许让 app-server 接管真实会话：

- fake app-server 契约测试稳定通过。
- bundled/system Codex 版本不匹配时能明确拒绝启动。
- DeepSeek Responses smoke test 能完成一次 function call 回环。
- 审批、取消和崩溃恢复均有自动化测试。
- 默认 0.9.5 路径回归测试无变化。

