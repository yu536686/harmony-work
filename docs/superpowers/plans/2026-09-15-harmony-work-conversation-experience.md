# Harmony Work 品牌与对话执行体验实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法跟踪进度。每轮开始和结束必须同步更新 `docs/roadmaps/harmony-work-v1.3-execution-ledger.md`。

**目标：** 将产品用户可见品牌统一为 Harmony Work，同时保留默认 Agent 名称“小亨”，并把主会话升级为可流式呈现公开思考摘要、结构化执行步骤、工具详情、成果预览和直接批注闭环的工作界面。

**架构：** 保留 Codex app-server 作为唯一主会话运行时，在 Rust 事件适配层增加公开 reasoning summary 与命令输出事件，并将经过脱敏的结构事件持久化。前端新增纯函数 `TurnExecutionModel` 聚合层，把实时事件与历史事件投影为同一套时间线；右侧 Workbench 复用成果、工具步骤和浏览器目标数据，并通过结构化批注上下文进入下一轮消息。产品品牌常量与 Agent 人格常量分离，保留现有 Tauri identifier 和用户数据目录以支持原地升级。

**技术栈：** React 18、TypeScript 5.7、Redux Toolkit、Vitest、Testing Library、Lucide React、Tauri 2、Rust、SQLite、Codex app-server JSON-RPC、WPF Bootstrapper、NSIS、Windows PowerShell。

---

## 实施约束

- 权威规格：`docs/superpowers/specs/2026-09-15-harmony-work-conversation-experience-design.md`。
- 正式仓库：远端 Windows `E:\harmony-work`，分支 `feat/harmony-work-conversation-experience`。
- SMB `/Volumes/192.168.11.5/harmony-work` 只用于文件读写；测试、构建、打包在远端 Windows 通过 SSH 执行。
- 产品名为 **Harmony Work**；默认 Agent 名为 **小亨**，英文为 **Xiaoheng**。
- 主输入框中文占位符固定为“给小亨发送消息”。
- 只显示 `item/reasoning/summaryTextDelta` 的公开摘要；忽略 `item/reasoning/textDelta`，且不得启用 `show_raw_agent_reasoning`。
- 所有工具参数和输出进入 UI 或执行事件存储前必须脱敏并限制长度。
- Tauri `identifier` 保持 `com.harmony.xiaoheng-workbuddy`，避免升级后丢失现有配置与数据库。
- 不改变 Harvis、StaffDeck、账号同步、技能、记忆、IM、定时任务、项目与审批协议。
- 目标版本为 `1.3.0-alpha.1`；安装包和 `release/` 不提交 Git。

## 文件职责

### 新建

- `src/branding.ts`：产品名、Agent 名和可见品牌资源的唯一前端常量。
- `src/branding.test.ts`：验证产品与 Agent 命名不会混用。
- `scripts/branding-contract.test.mjs`：扫描 Tauri、安装器、README 和用户可见文案中的旧产品名。
- `src/components/Chat/execution/types.ts`：时间线、状态、公开摘要和结构事件类型。
- `src/components/Chat/execution/reducer.ts`：从 `AgentEventType` 纯函数聚合 `TurnExecutionModel`。
- `src/components/Chat/execution/reducer.test.ts`：乱序、重复、失败、取消和完成状态测试。
- `src/components/Chat/execution/presentation.ts`：工具名称、摘要、脱敏结果的前端投影。
- `src/components/Chat/execution/presentation.test.ts`：工具显示与敏感字段测试。
- `src/components/Chat/ExecutionTimeline.tsx`：对话内公开摘要和执行步骤组件。
- `src/components/Chat/ExecutionTimeline.test.tsx`：折叠、状态图标和键盘行为测试。
- `src/components/Chat/WorkbenchPanel.tsx`：成果、摘要、浏览器、终端四区右侧面板。
- `src/components/Chat/WorkbenchPanel.test.tsx`：标签切换、空状态和关闭恢复测试。
- `src/components/Chat/ArtifactPreview.tsx`：图片、文本、代码、网页和文件摘要预览。
- `src/components/Chat/annotation/types.ts`：批注目标、选区和发送上下文类型。
- `src/components/Chat/annotation/reducer.ts`：会话级批注草稿状态机。
- `src/components/Chat/annotation/reducer.test.ts`：新增、移除、失败恢复和清理测试。
- `src/components/Chat/annotation/selection.ts`：文本偏移与归一化矩形计算。
- `src/components/Chat/annotation/selection.test.ts`：文本和图像选区边界测试。
- `src/components/Chat/AnnotationLayer.tsx`：成果上的选区、锚点和浮动编辑器。
- `src/components/Chat/AnnotationLayer.test.tsx`：框选、暂存、Esc 和多批注测试。
- `src/components/Chat/ConversationExperience.css`：时间线、Workbench、预览和批注样式。
- `src-tauri/src/commands/artifact_preview.rs`：受工作区权限约束的文本/图片预览读取命令。

### 修改

- `src/App.tsx`、`src/App.css`：Harmony Work 文字标识和侧边栏品牌。
- `src/components/About/index.tsx`、`src/components/Onboarding/index.tsx`、`src/components/Bootstrap/index.tsx`、`src/components/Overlay/index.tsx`、`src/components/Pond/IDE/index.tsx`：可见产品品牌替换。
- `src/i18n/zh.ts`、`src/i18n/en.ts`：区分产品名称与 Agent 名称，新增时间线和工作台文案。
- `src/components/Chat/index.tsx`：接入执行模型、时间线、Workbench 和批注发送；移除顶部四标签主入口。
- `src/components/Chat/Chat.css`：删除旧任务面板布局，只保留尚未迁出的兼容样式。
- `src/store/slices/chat.ts`、`src/store/slices/chat.test.ts`：增加会话执行模型与批注草稿状态。
- `src/services/tauri/chat.ts`、`src/services/tauri/tauri.test.ts`：扩展公开执行事件、历史执行记录、成果预览和批注上下文 IPC 类型。
- `src-tauri/src/codex_runtime/event_adapter.rs`：映射公开 reasoning summary、工具输出增量并明确丢弃 raw reasoning。
- `src-tauri/src/codex_runtime/service.rs`：持久化脱敏结构事件，分离可见用户消息与模型上下文。
- `src-tauri/src/store/db.rs`：增加消息 `context_json` 与 `turn_execution_events` 迁移和查询方法。
- `src-tauri/src/commands/chat.rs`、`src-tauri/src/app/bootstrap.rs`：注册历史执行和成果预览命令。
- `src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`：版本和用户可见应用元数据。
- `public/logo.png`、`public/piscis.png`、`src-tauri/icons/*`：替换为确认的 Harmony Work 图标资产；旧 URL 文件名暂时保留以避免遗漏引用。
- `installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj`、`MainWindow.xaml`、`MainWindow.xaml.cs`、`InstallerEngine.cs`、`app.manifest`：安装器品牌、程序集、快捷方式和启动路径。
- `scripts/build-full-installer.ps1`、`scripts/stage-full-installer-payload.ps1`、`scripts/offline-runtime-lock.test.mjs`：新安装包名称和载荷断言。
- `package.json`、`package-lock.json`、`README.md`：版本、项目名、仓库链接和发行说明。
- `docs/roadmaps/harmony-work-v1.3-execution-ledger.md`：逐任务检查点和验证证据。

## 任务 1：固化规格与品牌命名契约

**文件：**
- 创建：`src/branding.ts`
- 创建：`src/branding.test.ts`
- 创建：`scripts/branding-contract.test.mjs`
- 修改：`package.json`
- 修改：`docs/roadmaps/harmony-work-v1.3-execution-ledger.md`

- [x] **步骤 1：为品牌常量编写失败测试**

```ts
import { describe, expect, it } from "vitest";
import { AGENT_BRAND, PRODUCT_BRAND } from "./branding";

describe("branding contract", () => {
  it("separates the product brand from the default agent identity", () => {
    expect(PRODUCT_BRAND.displayName).toBe("Harmony Work");
    expect(AGENT_BRAND.displayNameZh).toBe("小亨");
    expect(AGENT_BRAND.composerPlaceholderZh).toBe("给小亨发送消息");
  });
});
```

- [x] **步骤 2：运行测试并确认失败**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/branding.test.ts"'`

预期：FAIL，提示无法解析 `./branding`。

- [x] **步骤 3：实现唯一品牌常量**

```ts
export const PRODUCT_BRAND = {
  displayName: "Harmony Work",
  shortName: "Harmony Work",
  publisher: "Harmony",
  iconPath: "/harmony-work-icon.png",
} as const;

export const AGENT_BRAND = {
  displayNameZh: "小亨",
  displayNameEn: "Xiaoheng",
  composerPlaceholderZh: "给小亨发送消息",
  composerPlaceholderEn: "Message Xiaoheng",
} as const;
```

- [x] **步骤 4：增加仓库级可见品牌扫描**

`scripts/branding-contract.test.mjs` 读取 `src/i18n`、`src/App.tsx`、Onboarding、About、Overlay、Bootstrapper、`tauri.conf.json` 和 `README.md`，断言用户可见区域不再包含 `小亨 WorkBuddy`、旧 GitHub 仓库 URL 或旧安装包名；允许 `小亨`、`Xiaoheng` 出现在 Agent 人格文案中，允许内部兼容路径和 Tauri identifier 保留旧标识。

品牌迁移完成前先将扫描器作为独立预期失败测试运行；任务 3 清理完安装器与发行元数据后，再在 `package.json` 的 `test` 尾部加入：

```json
"test": "vitest run && node --test scripts/offline-runtime-lock.test.mjs scripts/branding-contract.test.mjs"
```

- [x] **步骤 5：运行测试并确认通过**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/branding.test.ts; node --test scripts/branding-contract.test.mjs"'`

预期：品牌单测 PASS；独立执行品牌扫描时仍因尚未迁移的可见文案 FAIL，并准确列出文件。默认 `npm test` 暂不接入该扫描器，避免阶段检查点长期红灯。

- [x] **步骤 6：提交检查点**

```powershell
git add src/branding.ts src/branding.test.ts scripts/branding-contract.test.mjs package.json docs/roadmaps/harmony-work-v1.3-execution-ledger.md
git commit -m "test: 固化 Harmony Work 品牌命名契约"
```

## 任务 2：迁移应用内可见品牌与图标

**文件：**
- 修改：`src/App.tsx`
- 修改：`src/App.css`
- 修改：`src/components/About/index.tsx`
- 修改：`src/components/Onboarding/index.tsx`
- 修改：`src/components/Bootstrap/index.tsx`
- 修改：`src/components/Overlay/index.tsx`
- 修改：`src/components/Pond/IDE/index.tsx`
- 修改：`src/i18n/zh.ts`
- 修改：`src/i18n/en.ts`
- 修改：`public/logo.png`
- 修改：`public/piscis.png`
- 创建：`public/harmony-work-icon.png`
- 创建：`public/harmony-work-wordmark.png`
- 修改：`src-tauri/icons/*`

- [x] **步骤 1：复制并校验用户确认的品牌源文件**

源文件：

```text
/Users/yutianqi/Downloads/未标题-1.png
/Users/yutianqi/Downloads/ChatGPT Image 2026年9月15日 10_59_44.png
```

将原始 RGBA 图标保存为 `public/harmony-work-icon.png`，文字标识保存为 `public/harmony-work-wordmark.png`。用 `npx tauri icon public/harmony-work-icon.png` 生成 Windows ICO、托盘与各尺寸图标，再将兼容 URL `public/logo.png`、`public/piscis.png` 指向同一视觉资产内容。

- [x] **步骤 2：更新应用壳层品牌**

`App.tsx` 左上角使用图标和可缩放文字标识；可访问文本为 `Harmony Work`。About、Onboarding、Bootstrap 和 IDE 欢迎页使用 `PRODUCT_BRAND`，Overlay 标题显示 `Harmony Work`。

Agent 对话相关字符串继续使用 `AGENT_BRAND`：

```tsx
<textarea placeholder={AGENT_BRAND.composerPlaceholderZh} />
<div className="message-role">{AGENT_BRAND.displayNameZh}</div>
```

- [x] **步骤 3：迁移中英文文案**

规则：

```text
产品/应用/安装器/窗口/关于页：Harmony Work
对话助手/记忆主体/任务执行者：小亨 / Xiaoheng
内部 WorkBuddy MCP Host：保留协议名，不作为用户主品牌展示
```

- [x] **步骤 4：运行品牌、设计和应用测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/branding.test.ts src/utils/designTokens.test.ts"'`

预期：全部 PASS，品牌扫描仅剩安装器与发行元数据项。

- [x] **步骤 5：执行前端构建**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npm run build:web"'`

预期：TypeScript 和 Vite 构建成功，`dist` 中包含新品牌资产。

- [x] **步骤 6：提交检查点**

```powershell
git add src public src-tauri/icons
git commit -m "feat: 更新 Harmony Work 应用品牌与图标"
```

## 任务 3：升级版本、Windows 应用元数据与安装器品牌

**文件：**
- 修改：`package.json`
- 修改：`package-lock.json`
- 修改：`src-tauri/tauri.conf.json`
- 修改：`src-tauri/Cargo.toml`
- 修改：`src-tauri/Cargo.lock`
- 修改：`installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj`
- 修改：`installer/WorkBuddyBootstrapper/MainWindow.xaml`
- 修改：`installer/WorkBuddyBootstrapper/MainWindow.xaml.cs`
- 修改：`installer/WorkBuddyBootstrapper/InstallerEngine.cs`
- 修改：`installer/WorkBuddyBootstrapper/app.manifest`
- 修改：`scripts/build-full-installer.ps1`
- 修改：`scripts/stage-full-installer-payload.ps1`
- 修改：`scripts/offline-runtime-lock.test.mjs`
- 修改：`README.md`

- [x] **步骤 1：扩展安装器失败测试**

在 `scripts/offline-runtime-lock.test.mjs` 增加断言：最终文件名为 `Harmony-Work-1.3.0-alpha.1-x64-setup.exe`，WPF AssemblyName 为 `Harmony-Work-Setup`，快捷方式名和卸载项为 `Harmony Work`，载荷启动文件存在且使用绝对路径。

- [x] **步骤 2：运行安装器测试并确认失败**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npm run test:installer"'`

预期：FAIL，显示旧 `Xiaoheng-WorkBuddy` 文件名或旧产品名。

- [x] **步骤 3：更新版本和用户可见元数据**

统一版本为 `1.3.0-alpha.1`。`tauri.conf.json` 的 `productName`、窗口标题、描述改为 Harmony Work；保留 `identifier: com.harmony.xiaoheng-workbuddy`。将 `Cargo.toml` 的 package/bin 名称和 `tauri.conf.json` 的 `mainBinaryName` 统一为 `harmony-work`，Rust library crate 名保持兼容。内部数据库目录不迁移，安装器同时识别 `harmony-work.exe` 与旧 `xiaoheng-workbuddy.exe`，复用现有安装目录并在升级完成后移除失效快捷方式。

- [x] **步骤 4：更新 WPF Bootstrapper**

`InstallerEngine` 使用：

```csharp
private const string ProductName = "Harmony Work";
private const string ApplicationExecutable = "harmony-work.exe";
```

完成页“立即启动”启动新可执行文件后调用 `Application.Current.Shutdown()`；旧版本可执行文件仅作为升级检测兼容项，不创建旧品牌快捷方式。

- [x] **步骤 5：更新 README 和 GitHub 链接**

仓库、issues、releases 全部指向 `yu536686/harmony-work`；下载与校验示例使用新安装包名。说明 Agent 仍名为小亨。

将 `scripts/branding-contract.test.mjs` 接入 `package.json` 的默认 `test`，从本步骤开始全量测试必须同时满足品牌扫描。

- [x] **步骤 6：运行安装器测试和 WPF 编译**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npm run test:installer; dotnet build installer\WorkBuddyBootstrapper\WorkBuddyBootstrapper.csproj -c Release -p:Platform=x64"'`

预期：Node 安装器测试 PASS，WPF 0 error。

- [x] **步骤 7：提交检查点**

```powershell
git add package.json package-lock.json src-tauri installer scripts README.md
git commit -m "feat: 升级 Harmony Work 1.3 安装与发行品牌"
```

## 任务 4：扩展 Codex 公开执行事件并脱敏

**文件：**
- 修改：`src-tauri/src/codex_runtime/event_adapter.rs`
- 修改：`src/services/tauri/chat.ts`
- 修改：`src/services/tauri/tauri.test.ts`

- [x] **步骤 1：为公开 reasoning summary 编写 Rust 失败测试**

```rust
#[test]
fn streams_reasoning_summary_but_drops_raw_reasoning() {
    let mut adapter = CodexEventAdapter::default();
    let summary = adapter.adapt_notification(&notification(
        "item/reasoning/summaryTextDelta",
        json!({"itemId":"r1","summaryIndex":0,"delta":"正在核对素材"}),
    ));
    assert!(matches!(summary.as_slice(), [WorkBuddyEvent::ReasoningSummaryDelta { id, delta, .. }] if id == "r1" && delta == "正在核对素材"));

    let raw = adapter.adapt_notification(&notification(
        "item/reasoning/textDelta",
        json!({"itemId":"r1","contentIndex":0,"delta":"hidden"}),
    ));
    assert!(raw.is_empty());
}
```

- [x] **步骤 2：为命令输出增量和脱敏编写失败测试**

```rust
#[test]
fn redacts_sensitive_tool_fields_and_limits_output() {
    let event = completed_mcp_tool(json!({"api_key":"secret","result":"ok"}));
    let adapted = CodexEventAdapter::default().adapt_notification(&event);
    let json = serde_json::to_string(&adapted).unwrap();
    assert!(!json.contains("secret"));
    assert!(json.contains("[REDACTED]"));
    assert!(json.len() < 20_000);
}
```

- [x] **步骤 3：运行 Rust 测试并确认失败**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work\src-tauri; cargo test codex_runtime::event_adapter -- --nocapture"'`

预期：FAIL，缺少 `ReasoningSummaryDelta`。

- [x] **步骤 4：实现公开事件映射**

新增事件：

```rust
ReasoningSummaryStart { id: String, summary_index: u32 },
ReasoningSummaryDelta { id: String, summary_index: u32, delta: String },
ToolOutputDelta { id: String, delta: String },
```

映射 `item/reasoning/summaryPartAdded`、`item/reasoning/summaryTextDelta`、`item/commandExecution/outputDelta`。`item/reasoning/textDelta` 明确返回空数组。对工具参数、结果、命令输出执行递归敏感键脱敏、UTF-8 安全截断和 ANSI 控制符清理。

- [x] **步骤 5：同步 TypeScript 事件联合类型和 IPC 测试**

`AgentEventType` 增加同名 snake_case 事件，`tauri.test.ts` 断言 `chatApi.onEvent` 不改变通道名且 `chatApi.send` 保持现有参数兼容。

- [x] **步骤 6：运行 Rust 与前端服务测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; cargo test --manifest-path src-tauri\Cargo.toml codex_runtime::event_adapter; npx vitest run src/services/tauri/tauri.test.ts"'`

预期：全部 PASS。

- [x] **步骤 7：提交检查点**

```powershell
git add src-tauri/src/codex_runtime/event_adapter.rs src/services/tauri/chat.ts src/services/tauri/tauri.test.ts
git commit -m "feat: 接入 Codex 公开执行摘要事件"
```

## 任务 5：持久化结构化执行历史和批注上下文

**文件：**
- 修改：`src-tauri/src/store/db.rs`
- 修改：`src-tauri/src/codex_runtime/service.rs`
- 修改：`src-tauri/src/commands/chat.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`
- 修改：`src/services/tauri/chat.ts`

- [x] **步骤 1：编写数据库迁移与往返失败测试**

```rust
#[test]
fn execution_events_and_message_context_roundtrip() {
    let db = test_db();
    let session = db.create_session(Some("test"), "chat").unwrap();
    db.append_turn_execution_event(&session.id, "turn-1", 1, "reasoning_summary", r#"{"text":"核对素材"}"#).unwrap();
    db.append_message_with_context(&session.id, "user", "请修改", Some(r#"[{"targetType":"artifact"}]"#)).unwrap();
    assert_eq!(db.list_turn_execution_events(&session.id, 20).unwrap().len(), 1);
    assert!(db.get_messages(&session.id, 20, 0).unwrap()[0].context_json.is_some());
}
```

- [x] **步骤 2：运行数据库测试并确认失败**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work\src-tauri; cargo test store::db::tests::execution_events_and_message_context_roundtrip"'`

预期：FAIL，缺少迁移和方法。

- [x] **步骤 3：增加向后兼容迁移**

为 `messages` 增加可空 `context_json TEXT`。新增：

```sql
CREATE TABLE IF NOT EXISTS turn_execution_events (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  sequence INTEGER NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(turn_id, sequence),
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_turn_execution_session
ON turn_execution_events(session_id, created_at, sequence);
```

- [x] **步骤 4：分离可见用户内容和模型上下文**

`codex_chat_send` 保存原始 `content` 到消息正文，把通过 schema/数量/长度校验的批注 JSON 保存到 `context_json`。传给 Codex 的内容仍由 `compose_turn_content` 合成，但不得把 `<workbuddy_user_context>` 写入用户可见正文。

- [x] **步骤 5：只持久化结构事件**

保存 turn start、公开摘要完成片段、plan update、脱敏 tool start/end、done/cancel/error；不保存 `text_delta`、raw reasoning 和高频 command output delta。每轮事件上限 500 条，单 payload 上限 16 KiB。

- [x] **步骤 6：暴露历史查询 IPC**

```ts
export interface TurnExecutionEventRecord {
  turn_id: string;
  sequence: number;
  event_type: string;
  payload: unknown;
  created_at: string;
}

export const chatApi = {
  // existing methods
  getExecutionHistory: (sessionId: string, limit = 500) =>
    invoke<TurnExecutionEventRecord[]>("get_turn_execution_history", { sessionId, limit }),
};
```

- [x] **步骤 7：运行数据库、Codex service 与 IPC 测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; cargo test --manifest-path src-tauri\Cargo.toml store::db; cargo test --manifest-path src-tauri\Cargo.toml codex_runtime::service; npx vitest run src/services/tauri/tauri.test.ts"'`

预期：全部 PASS，旧数据库迁移测试保留通过。

- [x] **步骤 8：提交检查点**

```powershell
git add src-tauri/src/store/db.rs src-tauri/src/codex_runtime/service.rs src-tauri/src/commands/chat.rs src-tauri/src/app/bootstrap.rs src/services/tauri/chat.ts
git commit -m "feat: 持久化会话执行轨迹与批注上下文"
```

## 任务 6：建立前端 TurnExecutionModel 聚合层

**文件：**
- 创建：`src/components/Chat/execution/types.ts`
- 创建：`src/components/Chat/execution/reducer.ts`
- 创建：`src/components/Chat/execution/reducer.test.ts`
- 创建：`src/components/Chat/execution/presentation.ts`
- 创建：`src/components/Chat/execution/presentation.test.ts`
- 修改：`src/store/slices/chat.ts`
- 修改：`src/store/slices/chat.test.ts`

- [x] **步骤 1：定义执行模型测试夹具**

```ts
const initial = createTurnExecutionModel("session-1");
const started = reduceExecutionEvent(initial, { type: "run_status", iteration: 1, phase: "thinking", detail: "正在处理任务" });
const planned = reduceExecutionEvent(started, { type: "plan_update", items: [{ id: "step-1", content: "读取文件", status: "in_progress" }] });
const running = reduceExecutionEvent(planned, { type: "tool_start", id: "tool-1", name: "shell", input: { command: "npm test" } });
const done = reduceExecutionEvent(running, { type: "tool_end", id: "tool-1", name: "shell", result: "PASS", is_error: false });

expect(done.steps[0].state).toBe("completed");
expect(done.steps[0].expanded).toBe(false);
```

- [x] **步骤 2：增加边界测试**

覆盖：重复事件幂等、tool end 先于 start、失败、取消、权限等待、结束后清除运行状态、多轮隔离、历史事件重放、工具输出截断。

- [x] **步骤 3：运行测试并确认失败**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/components/Chat/execution/*.test.ts"'`

预期：FAIL，聚合模块不存在。

- [x] **步骤 4：实现纯函数状态机**

核心类型：

```ts
export type ExecutionState = "idle" | "running" | "awaiting_approval" | "completed" | "failed" | "cancelled";
export type StepState = "pending" | "running" | "awaiting_approval" | "completed" | "failed" | "cancelled";

export interface TurnExecutionModel {
  sessionId: string;
  turnId?: string;
  state: ExecutionState;
  iteration: number;
  reasoning: Array<{ id: string; summaryIndex: number; text: string; streaming: boolean }>;
  steps: ExecutionStep[];
  plan: PlanTodoItem[];
  finalText: string;
  startedAt?: string;
  completedAt?: string;
}
```

- [x] **步骤 5：接入 Redux 会话状态**

增加 `executionBySession`，新增 `applyExecutionEvent`、`restoreExecutionHistory`、`toggleExecutionStep`。旧 `toolSteps`、`planBySession` 暂时保留供其他组件兼容，但主会话 UI 改读新模型；下一轮开始只重置当前执行模型，不影响历史消息。

- [x] **步骤 6：运行聚合层和 store 测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/components/Chat/execution src/store/slices/chat.test.ts"'`

预期：全部 PASS。

- [x] **步骤 7：提交检查点**

```powershell
git add src/components/Chat/execution src/store/slices/chat.ts src/store/slices/chat.test.ts
git commit -m "feat: 建立统一会话执行状态模型"
```

## 任务 7：实现对话内执行时间线

**文件：**
- 创建：`src/components/Chat/ExecutionTimeline.tsx`
- 创建：`src/components/Chat/ExecutionTimeline.test.tsx`
- 创建：`src/components/Chat/ConversationExperience.css`
- 修改：`src/components/Chat/index.tsx`
- 修改：`src/components/Chat/Chat.css`
- 修改：`src/i18n/zh.ts`
- 修改：`src/i18n/en.ts`

- [x] **步骤 1：编写运行态和完成态组件测试**

```tsx
render(<ExecutionTimeline model={modelWithRunningAndCompletedSteps} onToggleStep={onToggle} />);
expect(screen.getByText("读取并整理素材")).toBeVisible();
expect(screen.getByLabelText("已完成")).toBeVisible();
expect(screen.getByLabelText("执行中")).toBeVisible();
expect(screen.queryByText("npm test -- --run")).not.toBeVisible();
await user.click(screen.getByRole("button", { name: /生成并验证/ }));
expect(screen.getByText("npm test -- --run")).toBeVisible();
```

- [x] **步骤 2：测试公开摘要而非隐藏推理**

组件只接受 `model.reasoning[].text`；测试夹具中的 raw 字段即使存在于未知对象，也不能渲染。完成后摘要折叠为“思考过程”，当前流式摘要带轻量动画并尊重 `prefers-reduced-motion`。

- [x] **步骤 3：实现时间线和状态图标**

使用 Lucide `CircleCheck`、`LoaderCircle`、`CircleX`、`ShieldAlert`、`CircleStop`、`ChevronRight`。图标按钮均有中文 `aria-label` 和 `title`。完成步骤默认折叠，当前步骤默认展开，用户手动状态在该回合内保持。

- [x] **步骤 4：替换旧顶部 Activity/Todo/Tools/Artifacts 面板**

删除 `chat-topbar-tasks` 作为主入口，将 plan、tools、run status 统一放入消息流中的 `ExecutionTimeline`。顶部右侧只保留 Workbench 开关。审批弹窗沿用现有状态和协议，不嵌入时间线内部。

- [x] **步骤 5：接入实时和历史执行事件**

`onEvent` 首先 dispatch `applyExecutionEvent`，然后保留现有消息、审批、artifact 刷新副作用。会话切换时并发获取 messages、artifacts、execution history，并通过 `restoreExecutionHistory` 重建静态时间线。

- [x] **步骤 6：运行组件和主 store 测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/components/Chat/ExecutionTimeline.test.tsx src/store/slices/chat.test.ts"'`

预期：全部 PASS。

- [x] **步骤 7：执行前端构建**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npm run build:web"'`

预期：构建成功，无未使用旧任务面板状态。

- [x] **步骤 8：提交检查点**

```powershell
git add src/components/Chat src/i18n src/store/slices/chat.ts
git commit -m "feat: 重构对话执行时间线"
```

## 任务 8：实现右侧 Workbench 与成果预览

**文件：**
- 创建：`src/components/Chat/WorkbenchPanel.tsx`
- 创建：`src/components/Chat/WorkbenchPanel.test.tsx`
- 创建：`src/components/Chat/ArtifactPreview.tsx`
- 创建：`src-tauri/src/commands/artifact_preview.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`
- 修改：`src/services/tauri/chat.ts`
- 修改：`src/components/Chat/index.tsx`
- 修改：`src/components/Chat/ConversationExperience.css`

- [x] **步骤 1：编写 Workbench 标签和空状态失败测试**

```tsx
render(<WorkbenchPanel open artifacts={[artifact]} execution={execution} onClose={onClose} />);
expect(screen.getByRole("tab", { name: "成果" })).toHaveAttribute("aria-selected", "true");
await user.click(screen.getByRole("tab", { name: "终端" }));
expect(screen.getByText(/Verification passed/)).toBeVisible();
```

- [x] **步骤 2：编写成果预览权限失败测试**

Rust 测试覆盖：工作区内文本可读取；工作区外且未授权时拒绝；超过 1 MiB 返回摘要和 `truncated=true`；二进制不作为文本返回；路径不存在返回可操作错误。

- [x] **步骤 3：实现成果预览 IPC**

```ts
export interface ArtifactPreviewPayload {
  kind: "image" | "text" | "code" | "html" | "external";
  uri: string;
  content?: string;
  media_type?: string;
  truncated: boolean;
}
```

命令以会话工作区和 `allow_outside_workspace` 为权限边界，复用规范化路径校验。图片用 `convertFileSrc`；文本/代码最多读取 1 MiB；Office 显示摘要、路径及“打开/另存为/在文件夹中显示”，不承诺伪造 PPT 渲染。

- [x] **步骤 4：实现四区 Workbench**

- 成果：最近更新的 artifacts 列表和预览。
- 摘要：从 `TurnExecutionModel` 纯函数生成目标、完成项、失败项和成果，不调用模型。
- 浏览器：从 `web_search`、`web_fetch`、`browser` 工具步骤提取 URL、标题、状态和可用截图。
- 终端：从 `shell`/`powershell` 步骤显示脱敏命令和最近输出，只读，不新建 Shell。

- [x] **步骤 5：实现响应式布局**

宽度大于 1180px 时为 `sidebar + chat + 420px panel`；980-1180px 使用 360px panel；小于 980px Workbench 作为右侧覆盖层并提供关闭按钮。面板开关状态按会话保留，不能改变主输入框宽高。

- [x] **步骤 6：运行前后端测试与构建**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; cargo test --manifest-path src-tauri\Cargo.toml artifact_preview; npx vitest run src/components/Chat/WorkbenchPanel.test.tsx; npm run build:web"'`

预期：全部 PASS。

- [x] **步骤 7：提交检查点**

```powershell
git add src/components/Chat src/services/tauri/chat.ts src-tauri/src/commands/artifact_preview.rs src-tauri/src/app/bootstrap.rs
git commit -m "feat: 增加会话右侧工作台与成果预览"
```

## 任务 9：实现成果内直接选区与批注草稿

**文件：**
- 创建：`src/components/Chat/annotation/types.ts`
- 创建：`src/components/Chat/annotation/reducer.ts`
- 创建：`src/components/Chat/annotation/reducer.test.ts`
- 创建：`src/components/Chat/annotation/selection.ts`
- 创建：`src/components/Chat/annotation/selection.test.ts`
- 创建：`src/components/Chat/AnnotationLayer.tsx`
- 创建：`src/components/Chat/AnnotationLayer.test.tsx`
- 修改：`src/components/Chat/ArtifactPreview.tsx`
- 修改：`src/components/Chat/WorkbenchPanel.tsx`
- 修改：`src/components/Chat/ConversationExperience.css`

- [x] **步骤 1：定义批注数据模型测试**

`WorkbenchAnnotation` 是前端可编辑草稿；`toCommentContext(annotation)` 在发送前把它转换为 IPC 使用的 `WorkbenchCommentContext`。两者共享 target、selection 和 comment 字段，但草稿额外包含 UI 状态，避免把界面状态发送给模型。

```ts
const annotation: WorkbenchAnnotation = {
  id: "annotation-1",
  sessionId: "session-1",
  target: { type: "artifact", id: "artifact-1", title: "方案.pptx", uri: "E:/work/方案.pptx" },
  selection: { type: "image_region", page: 1, x: 0.18, y: 0.27, width: 0.62, height: 0.47 },
  comment: "标题更简洁，主视觉换成腕表微距",
  status: "staged",
};
expect(addAnnotation(emptyDraft, annotation).items).toHaveLength(1);
expect(toCommentContext(annotation)).not.toHaveProperty("status");
```

- [x] **步骤 2：测试选区边界**

矩形从任意拖动方向计算，所有坐标 clamp 到 0-1；小于 4px 的拖动视为点注释；文本选择保存字符偏移、行号和最多 500 字 excerpt。

- [x] **步骤 3：测试交互闭环**

进入“注释”模式后按钮显示“正在注释”；框选成果后浮层贴近选区；“添加到对话”生成编号锚点和 composer 标签但不发送；`Esc` 只取消当前未确认选区；切换标签或关闭 Workbench 后已暂存批注仍存在。

- [x] **步骤 4：实现 AnnotationLayer**

图片/幻灯片/视频帧使用 Pointer Events 捕获拖动；文本和代码监听 `selectionchange` 并限制在预览根节点。浮层位置通过选区 bounding rect 与面板边界计算，不遮挡选区且不超出视口。

- [x] **步骤 5：实现多批注暂存**

每条批注以 `批注 N · 页码/行号/目标` 标签显示在主 composer 上方。删除标签同时移除对应锚点；编辑标签重新定位预览并打开浮层；发送失败时标记 `failed` 但不丢失内容。

- [x] **步骤 6：运行批注单测和组件测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/components/Chat/annotation src/components/Chat/AnnotationLayer.test.tsx"'`

预期：全部 PASS。

- [x] **步骤 7：提交检查点**

```powershell
git add src/components/Chat/annotation src/components/Chat/AnnotationLayer.tsx src/components/Chat/AnnotationLayer.test.tsx src/components/Chat/ArtifactPreview.tsx src/components/Chat/WorkbenchPanel.tsx src/components/Chat/ConversationExperience.css
git commit -m "feat: 支持成果直接选区与批注"
```

## 任务 10：把批注作为结构化上下文发送给小亨

**文件：**
- 修改：`src/services/tauri/chat.ts`
- 修改：`src/services/tauri/tauri.test.ts`
- 修改：`src/components/Chat/index.tsx`
- 修改：`src-tauri/src/codex_runtime/service.rs`

- [x] **步骤 1：编写前端发送失败测试**

```ts
await expect(sendAnnotatedMessage({ content: "按批注修改", annotations })).rejects.toThrow();
expect(store.getState().chat.annotationDrafts["session-1"].items).toHaveLength(2);
expect(store.getState().chat.annotationDrafts["session-1"].sendState).toBe("failed");
```

- [x] **步骤 2：扩展 `ChatSendOptions`**

```ts
export interface ChatSendOptions {
  // existing fields
  commentContexts?: WorkbenchCommentContext[];
}
```

IPC 参数名使用 `commentContexts`，Rust 接收 `comment_contexts`。每轮最多 20 条，每条评论最多 2000 字，excerpt 最多 500 字，URI 最多 2048 字，坐标必须在 0-1。

- [x] **步骤 3：构造模型上下文但保持消息正文干净**

模型收到：

```xml
<workbuddy_artifact_annotations>
[{"targetType":"artifact","title":"方案.pptx","selection":{"type":"image_region","page":1,"x":0.18,"y":0.27,"width":0.62,"height":0.47},"comment":"标题更简洁"}]
</workbuddy_artifact_annotations>
```

数据库消息正文只保存用户输入“按批注修改”，结构数据保存到 `context_json`，历史消息通过批注标签展示来源但不展开内部 XML。

- [x] **步骤 4：成功后清理、失败后保留**

`chatApi.send` 成功启动后将批注标记为发送中；收到本轮 `done` 才清空。同步抛错、运行期 `error` 或 `cancelled` 均保留全部草稿并标记失败，用户可直接重试或主动删除。

- [x] **步骤 5：运行前后端发送测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npx vitest run src/services/tauri/tauri.test.ts src/store/slices/chat.test.ts; cargo test --manifest-path src-tauri\Cargo.toml codex_runtime::service"'`

预期：全部 PASS，持久化正文不包含 `<workbuddy_artifact_annotations>`。

- [x] **步骤 6：提交检查点**

```powershell
git add src/services/tauri/chat.ts src/services/tauri/tauri.test.ts src/components/Chat/index.tsx src/store/slices/chat.ts src/store/slices/chat.test.ts src-tauri/src/codex_runtime/service.rs src-tauri/src/store/db.rs
git commit -m "feat: 将成果批注接入小亨下一轮任务"
```

## 任务 11：响应式、无障碍与视觉回归

**文件：**
- 修改：`src/components/Chat/ConversationExperience.css`
- 修改：`src/components/Chat/ExecutionTimeline.test.tsx`
- 修改：`src/components/Chat/WorkbenchPanel.test.tsx`
- 修改：`src/components/Chat/AnnotationLayer.test.tsx`
- 修改：`src/utils/designTokens.test.ts`
- 修改：`docs/roadmaps/harmony-work-v1.3-execution-ledger.md`

- [x] **步骤 1：补充键盘与 reduced-motion 测试**

验证 Enter/Space 展开步骤、Tab 可到达 Workbench 标签和批注操作、Esc 退出当前批注、焦点返回触发控件。`prefers-reduced-motion: reduce` 下旋转与脉冲动画禁用，但状态文字和图标仍可辨识。

- [x] **步骤 2：补充布局断言**

验证长中文文件名、长命令、长 URL、20 条批注、空成果和预览错误均不撑破容器；主输入框不遮挡审批弹窗或文件操作提示。

- [x] **步骤 3：运行前端全量测试**

运行：`ssh win 'powershell -NoProfile -Command "Set-Location E:\harmony-work; npm test"'`

预期：Vitest 和 Node tests 全部 PASS。

- [ ] **步骤 4：启动远端 Vite 并做截图验收**

在远端 Windows 启动可访问的 Vite 预览，使用 Playwright 检查至少：

```text
1440x900：执行中、已完成、失败、Workbench 成果、批注浮层
1024x768：窄面板布局、输入框与审批层不重叠
```

执行 canvas/图片像素非空检查，确认新图标、成果图片和状态图标实际渲染。

原生 Windows 客户端最小窗口宽度固定为 `980px`。小于该尺寸的 Workbench
覆盖层仍作为 CSS/组件契约维护，但不作为本次桌面客户端截图验收项；若未来
支持移动端或将最小宽度下调，必须新增独立响应式重构任务并重新恢复该项。

- [ ] **步骤 5：真实 Codex 回合冒烟**

在 Windows 客户端执行：读取两个文件、运行一条安全命令、生成一个文本成果、展开工具详情、添加两条批注并发送修改。确认公开摘要流式更新，raw reasoning 不出现，结束后所有 loading 状态停止。

- [ ] **步骤 6：提交检查点**

```powershell
git add src/components/Chat src/utils/designTokens.test.ts docs/roadmaps/harmony-work-v1.3-execution-ledger.md
git commit -m "test: 完成 Harmony Work 对话体验回归"
```

## 任务 12：远端 Windows 全量构建、升级安装与打包

**文件：**
- 修改：`docs/roadmaps/harmony-work-v1.3-execution-ledger.md`

- [ ] **步骤 1：确认干净工作树和版本一致性**

运行：

```powershell
git status --short
node -p "require('./package.json').version"
cargo metadata --manifest-path src-tauri\Cargo.toml --no-deps --format-version 1
```

预期：除台账验证记录外无未提交改动；JS、Tauri、Cargo 均为 `1.3.0-alpha.1`。

- [ ] **步骤 2：运行全量测试**

运行：

```powershell
npm test
npm run test:artifact-runtime
npm run test:codex-sdk
npm run test:codex-sidecar
npm run test:mcp-host
cargo test --manifest-path src-tauri\Cargo.toml --workspace
```

预期：所有命令退出码 0。

- [ ] **步骤 3：运行全量构建**

运行：

```powershell
npm run build
npx tauri build --bundles nsis
```

预期：Web、Codex sidecar、MCP Host、Artifact Runtime、Rust 主程序和 NSIS 全部构建成功。

- [ ] **步骤 4：构建完整 WPF Bootstrapper**

运行：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-full-installer.ps1 -Version 1.3.0-alpha.1
```

预期：生成 `Harmony-Work-1.3.0-alpha.1-x64-setup.exe`，预解压和安装进度可见，环境检测会跳过已安装私有运行时。

- [ ] **步骤 5：执行新装与升级冒烟**

在 Windows 测试账户分别验证从空环境安装、从 `1.2.0-alpha.7` 原地升级。检查：配置和数据库保留、旧快捷方式被替换、新图标生效、安装结束可立即启动且安装器退出、主会话和独立功能路由可用。

- [ ] **步骤 6：记录产物和哈希**

```powershell
Get-FileHash .\release\Harmony-Work-1.3.0-alpha.1-x64-setup.exe -Algorithm SHA256
Get-Item .\release\Harmony-Work-1.3.0-alpha.1-x64-setup.exe | Select-Object FullName,Length,LastWriteTime
```

把路径、大小、SHA-256、测试计数和已知限制写入执行台账，不提交安装包。

- [ ] **步骤 7：最终代码审查和提交**

运行：

```powershell
git diff --check
git status --short
git log --oneline --decorate -12
```

完成代码审查后提交台账：

```powershell
git add docs/roadmaps/harmony-work-v1.3-execution-ledger.md
git commit -m "docs: 记录 Harmony Work 1.3 验收结果"
```

- [ ] **步骤 8：推送功能分支**

```powershell
git push -u origin feat/harmony-work-conversation-experience
```

预期：远端分支更新成功；是否合并 `main`、创建 Release 由用户在验收安装包后明确决定。

## 规格覆盖自检

- 品牌名、图标、窗口、安装器、README：任务 1-3。
- Agent 自我认知仍为小亨、输入框文案：任务 1-2、7。
- 公开思考摘要与 raw reasoning 隔离：任务 4、6-7。
- 结构化步骤、默认折叠、状态图标：任务 6-7。
- 任务完成回复与 loading 终止：任务 6-7、11。
- 右侧成果、摘要、浏览器、终端：任务 8。
- 成果内选择、锚点、浮层、多批注：任务 9。
- 批注组合发送、持久化与失败恢复：任务 5、10。
- 现有功能不受影响、Windows 打包：任务 11-12。
- 多轮恢复与检查点：每项提交步骤和执行台账。

## 执行顺序

严格按任务 1 到任务 12 顺序实施。任务 4-6 是事件和状态基础，未通过测试前不得开始时间线 UI；任务 8 完成后才能开始批注层；任务 11 全量回归通过后才能打包。任何阶段出现无法修复的回归，更新台账为“阻塞”并记录复现命令，不跳过验证继续打包。
