# WorkBuddy 删除 Piscis Runtime 实施计划

> **目标：** 在 `codex/b1.0-codex-app-server` 分支中，以 Codex app-server、Codex 原生工具和 WorkBuddy MCP Host 完整替换 Piscis Agent Runtime。最终安装包不包含 Piscis crate、源码、二进制、数据库命名或运行时回退路径。

## 架构边界

- 桌面主会话由长驻 `codex app-server` 执行。
- 微信 iLink 与企业微信 WebSocket 继续作为独立传输层，不进入 MCP，也不由模型维持连接。
- IM、定时任务、CLI 和无头任务统一通过 Codex runtime 执行；实时 IM 优先复用 app-server thread，纯 CLI/批处理使用 `codex exec --json`。
- WorkBuddy 专有能力通过 `workbuddy-mcp-host` 暴露，MCP handler 只调用独立 domain service，不依赖 Tauri Window 或 Piscis Tool trait。
- Codex thread 是 Agent 上下文权威；WorkBuddy 数据库只保存 UI 投影、IM 绑定、业务实体和 artifact 索引。

## 阶段一：主会话切换

### 任务 1：建立前端 Codex chat 兼容适配器

**文件：**
- 修改 `src/services/tauri/chat.ts`
- 修改 `src/components/Chat/index.tsx`
- 新增/修改对应前端测试

**步骤：**
1. 为现有 `chatApi.send` 契约编写失败测试，要求默认调用 `codex_chat_send`。
2. 保留现有 `agent_event_<session>` 事件名和取消接口形状。
3. 删除公开 UI 对 `chat_send` 的生产调用。
4. 验证普通问候只创建一个 Codex turn，不调用旧 router/plan。

### 任务 2：补齐 Codex 事件与消息持久化

**文件：**
- 修改 `src-tauri/src/codex_runtime/event_adapter.rs`
- 修改 `src-tauri/src/codex_runtime/service.rs`
- 修改 `src-tauri/src/store/db.rs` 或拆分后的消息 repository

**步骤：**
1. 为 agent message 完成事件、artifact、tool result 和终止状态添加契约测试。
2. 将增量消息投影到现有 UI 协议。
3. 在 turn 完成后持久化最终助手消息及 token 使用量。
4. 失败、中断时清理 active turn 和审批状态。

## 阶段二：WorkBuddy MCP Host

### 任务 3：建立独立 MCP Host 进程

**文件：**
- 新增 `workbuddy-mcp-host/`
- 新增 MCP schema、stdio transport、domain registry 和测试
- 修改 `src-tauri/src/codex_runtime/config.rs`

**步骤：**
1. 建立 stdio MCP server，stdout 仅输出协议，日志写 stderr。
2. 每个调用校验 session、workspace roots、权限和路径。
3. 将 MCP server 注册到独立 `CODEX_HOME/config.toml`。
4. 使用 MCP client fixture 验证 initialize、tools/list 和 tools/call。

### 任务 4：迁移业务工具

**范围：**
- Harvis 图片、音乐和提示词助手
- Office Word、PowerPoint、Excel artifact
- Skill 检索、安装和同步
- 长期记忆搜索、追加和整理
- IM 查询、回复、文件发送，以及官方 `wecom-cli`
- StaffDeck 人员、任务和状态
- 素材库、营销看板及必要的 RobotZ/UIA 能力

**步骤：**
1. 从每个旧 Tool 中提取无 Piscis trait 的 domain function。
2. 为 domain function 添加正常、权限失败和外部服务失败测试。
3. 注册粒度明确的 MCP tools/resources。
4. 为 artifact 返回 resource link 和 WorkBuddy metadata。
5. 删除对应 `piscis_kernel::Tool` 实现和重复注册逻辑。

## 阶段三：后台入口迁移

### 任务 5：IM 切换

**文件：**
- 修改 `src-tauri/src/app/bootstrap.rs`
- 保留并整理 `src-tauri/src/gateway/wechat.rs`
- 保留并整理 `src-tauri/src/gateway/wecom.rs`
- 新增 `src-tauri/src/codex_runtime/im.rs`

**步骤：**
1. 为微信/企微消息到 Codex thread 的固定绑定编写测试。
2. 将媒体字节安全落盘并转为原生 Codex attachment。
3. 将队列和取消语义映射到 turn start/interrupt。
4. 将最终文本和 artifact 交给现有 Channel send。
5. 删除其他 IM 渠道注册与设置入口。

### 任务 6：定时、CLI 和无头任务切换

**文件：**
- 修改 `src-tauri/src/app/headless.rs`
- 修改 scheduler/automation 调用点
- 修改 StaffDeck/后台触发调用点
- 完善 `src-tauri/src/codex_runtime/headless.rs`

**步骤：**
1. 增加 `codex exec resume`、取消句柄、超时和事件投影测试。
2. 建立稳定的 WorkBuddy session 到 Codex thread 绑定。
3. 替换全部 `run_agent_headless` 调用。
4. 删除 Koi/heartbeat 中的旧 Agent 执行路径，保留必要业务调度数据。

## 阶段四：物理删除

### 任务 7：数据迁移

**文件：**
- 新增一次性数据库迁移
- 修改启动迁移与备份逻辑

**步骤：**
1. 将旧会话、IM 绑定、业务数据复制到 WorkBuddy 数据库。
2. 保留只读备份，不在运行时双写。
3. 为重复启动、部分迁移和回滚添加测试。

### 任务 8：删除 Piscis

**删除/修改：**
- 删除 `piscis-core`、`piscis-kernel`、`piscis-ide-tools` Cargo 依赖。
- 删除 `third_party/piscis-engine`。
- 删除旧 `chat_send` Agent Loop、LLM clients、`run_agent_headless`。
- 删除 `src-tauri/src/piscis`、Piscis heartbeat、Piscis pool/session 命名和回退开关。
- RobotZ 改为不启用 `piscis-kernel` feature。

**删除门槛：**
```bash
rg -n 'piscis-core|piscis_kernel|piscis-kernel|piscis_ide|third_party/piscis|run_agent_headless' \
  src src-tauri Cargo.lock package.json scripts
```
除数据迁移兼容常量和迁移测试外必须无结果；最终再删除兼容常量。

## 验证与打包

1. `npm test`
2. `npm run build:web`
3. `cargo test --manifest-path src-tauri/Cargo.toml`
4. MCP Host 单元与协议集成测试
5. 微信 iLink、企业微信、Harvis、Office、技能、记忆、StaffDeck 回归
6. Windows x64 离线安装包构建
7. 解包扫描不得包含 Piscis 名称、crate、数据库或二进制
8. 安装后运行进程和日志不得出现 Piscis runtime 或回退路径

