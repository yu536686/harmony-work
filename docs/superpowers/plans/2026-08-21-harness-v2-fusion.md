# Harness v2 融合实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 交付包含最新 UI、StaffDeck、Harvis 和 Tool Runtime v2 的小亨 WorkBuddy 0.9.1 Windows 安装包。

**架构：** 以 `c34a6cc` 为功能基线，合入已经验证的 0.9 Agent Loop 改动，再在 Piscis `Tool`/`ToolRegistry` 上增加兼容的运行元数据、延迟工具发现和结构化产物。前端继续使用现有统一 UI，通过 Activity 和 Artifacts 展示可审计执行状态。

**技术栈：** Rust、Tokio、Serde、Tauri 2、React 18、TypeScript、Redux Toolkit、Vitest、NSIS。

---

## 文件结构

- `third_party/piscis-engine/piscis-kernel/src/agent/tool.rs`：工具元数据、结构化产物和注册表曝光查询。
- `third_party/piscis-engine/piscis-kernel/src/tools/tool_search.rs`：延迟工具搜索与完整定义加载。
- `third_party/piscis-engine/piscis-kernel/src/tools/mod.rs`：注册核心工具搜索能力。
- `third_party/piscis-engine/piscis-kernel/src/agent/loop_.rs`：单 Step 调度、按元数据并发和运行事件。
- `third_party/piscis-engine/piscis-kernel/src/agent/messages.rs`：Activity 与工具发现事件。
- `src-tauri/src/tools/harvis_creative.rs`：Harvis 兼容入口及明确动作适配器。
- `src-tauri/src/host.rs`：Harvis 工具和宿主工具元数据注册。
- `src-tauri/src/commands/chat.rs`：直接/延迟工具定义组装和已选技能提升。
- `src/store/slices/chat.ts`：运行状态和 Artifact 状态。
- `src/components/Chat/index.tsx`：Activity、Artifacts 和 Harvis 结果呈现。
- `src/services/tauri/creative.test.ts`：Harvis 代理请求兼容性测试。
- `third_party/piscis-engine/piscis-kernel/src/tools/tool_search.rs` 内测试：检索、曝光和 schema 加载。

### 任务 1：合入 0.9 Harness 基线

- [ ] 合并 `276253f`，冲突时保留 `c34a6cc` 的 UI、Harvis、项目会话和 `design.md`。
- [ ] 将 npm、Cargo、Tauri 版本统一为 `0.9.1`。
- [ ] 运行 `npm test`，预期现有 37 个测试全部通过。
- [ ] 运行 `cargo test -p piscis-kernel plan_projection_warns_without_becoming_a_hard_circuit_breaker`，预期 PASS。
- [ ] 提交：`feat(agent): merge harness runtime into unified UI`。

### 任务 2：工具元数据和延迟发现

- [ ] 在 `agent/tool.rs` 先添加测试，断言默认工具为独占、直接曝光，显式延迟工具不会进入直接定义列表。
- [ ] 运行对应 Rust 测试并确认因缺少 `ToolMetadata`/曝光筛选而失败。
- [ ] 添加 `ToolExposure`、`ToolExecutionMode`、`ToolRisk`、`ToolMetadata` 及 `Tool::metadata()` 默认实现。
- [ ] 添加 `ToolRegistry::direct_tool_defs()`、`deferred_summaries()` 和 `tool_def_for()`。
- [ ] 新建 `tool_search`，输入 schema 固定为：

```json
{"type":"object","required":["query"],"properties":{"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":12}}}
```

- [ ] 使用名称、类别和最小描述进行不区分大小写的分词评分；返回匹配工具的完整定义。
- [ ] 运行 Rust 测试，预期新增测试全部 PASS。
- [ ] 提交：`feat(tools): add deferred discovery metadata`。

### 任务 3：按元数据调度工具

- [ ] 在 `agent/loop_.rs` 添加测试工具，验证两个 `Parallel` 工具可以重叠执行且结果顺序不变，`Exclusive` 工具形成屏障。
- [ ] 运行测试并确认当前串行路径导致并发断言失败。
- [ ] 将现有只读批处理判断替换为 `ToolExecutionMode`，并发上限继续使用 `READ_TOOL_MAX_CONCURRENCY = 4`。
- [ ] 保留批准、循环检测和结果回执顺序；取消时等待已启动调用收敛，不启动后续调用。
- [ ] 运行 Agent Loop、计划和工具测试，预期全部 PASS。
- [ ] 提交：`feat(agent): schedule tools by execution metadata`。

### 任务 4：Harvis 工具与结构化产物

- [ ] 添加测试，验证文生图不需要参考图、音乐接受单个 prompt、保存结果返回本地文件 Artifact。
- [ ] 运行测试并确认明确动作工具和 Artifact 字段尚不存在。
- [ ] 保留 `harvis_creative`，新增图片、音乐、状态和保存适配器；调用现有 `commands::marketing` 方法。
- [ ] 为查询设置 `Parallel + Read`，生成设置 `Exclusive + External`，保存设置 `Exclusive + Write`。
- [ ] 将下载结果映射为 `ToolArtifact`，前端通过现有 Artifacts 面板展示并提供打开/定位按钮。
- [ ] 运行 Rust 和前端 Harvis 测试，预期全部 PASS。
- [ ] 提交：`feat(harvis): expose typed creative agent tools`。

### 任务 5：聊天工具组装与兼容验证

- [ ] 添加测试，断言首轮只包含 Core 工具和 `tool_search`，用户选中 Harvis Skill 后对应工具可直接使用。
- [ ] 运行测试并确认当前全量 `tool_defs` 行为失败。
- [ ] 修改 `chat.rs` 使用直接定义；保留 Skill 依赖工具的按轮提升。
- [ ] 确保旧模型伪工具文本仍经过兼容解析，但结构化调用优先且不会把原始 DSML 输出给用户。
- [ ] 运行 `npm test`、`npm run build:web` 和目标 Rust 测试，预期全部 PASS。
- [ ] 提交：`feat(chat): use capability-aware tool exposure`。

### 任务 6：远端集成、编译和交付

- [ ] 将本地分支同步到 Windows 新 worktree `E:\xiaoheng-workbuddy-harness-v2`，不得修改其他 worktree。
- [ ] 运行 `npm ci`、`npm test`、`npm run build:web`。
- [ ] 运行 `cargo test -p piscis-kernel` 和 `cargo test --manifest-path src-tauri/Cargo.toml`。
- [ ] 运行 `npm run tauri -- build --bundles nsis`。
- [ ] 计算安装包 SHA-256，确认版本号、文件名、Harvis 资源和图标均已打包。
- [ ] 提交最终文档：`docs: record harness v2 verification`。
- [ ] 将 `codex/0.9-harness-v2` 写入远端 Git 工作树并报告安装包绝对路径。

