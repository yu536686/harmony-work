# Harmony Work v1.3 多轮执行台账

> 本文件是跨会话、跨用量窗口的恢复入口。任何继续开发的 Agent 必须先阅读本文件和关联设计规格，再读取 Git 状态。不得只依据对话摘要继续开发。

## 固定目标

将产品用户可见品牌更新为 **Harmony Work**，保留默认 Agent 人格名称 **小亨**，并完成对话执行时间线、状态图标、折叠工具过程、右侧工作面板和成果评论闭环。

## 权威规格

- `docs/superpowers/specs/2026-09-15-harmony-work-conversation-experience-design.md`

遇到对话摘要、临时说明和规格冲突时，以权威规格和用户最新明确指令为准。

## 不得遗漏

- 产品名是 Harmony Work；Agent 名仍是小亨。
- 输入框中文文案是“给小亨发送消息”。
- 不展示模型隐藏推理，只展示公开思考摘要和执行轨迹。
- 执行步骤默认折叠已完成项、展开当前项，并显示明确状态图标。
- 右侧面板包含成果、任务摘要、浏览器和终端。
- 浏览器、图片、PPT、视频帧、代码/文本内容支持直接选区批注；不使用预览下方常驻评论框。
- 批注先以编号锚点和结构化标签添加到主输入框，允许多条合并后统一发送给小亨。
- 不破坏现有 UI 路由、账号、项目、Harvis、StaffDeck、技能、记忆、IM 和安装器功能。
- 所有编译和打包在远端 Windows `E:\harmony-work` 执行。
- 源码文件可通过已挂载的 SMB 共享操作；PowerShell、测试、编译和安装器执行通过 SSH。
- 最终提交推送到 `https://github.com/yu536686/harmony-work`。

## 版本与分支

- 基线：`main@16bfe7b`
- 开发分支：`feat/harmony-work-conversation-experience`
- 目标版本序列：`1.3.0-alpha.x`

## 阶段台账

| 阶段 | 状态 | 目标 | 必要验证 |
| --- | --- | --- | --- |
| 0. 规格与实施计划 | 已完成 | 固化范围、组件边界、验收条件和恢复协议 | 规格自检、用户审查 |
| 1. 品牌资产与名称迁移 | 已完成 | Harmony Work 文字标识、全套图标、窗口/安装器/页面品牌 | 文案扫描、图标尺寸检查、升级安装 |
| 2. 执行事件聚合层 | 进行中 | TurnExecutionModel、历史重建、状态归并与脱敏 | TS/Rust 单测、乱序/重复事件测试 |
| 3. 对话时间线 UI | 未开始 | 流式摘要、步骤折叠、状态图标、最终回复样式 | 组件测试、运行/完成/失败截图 |
| 4. 右侧工具面板 | 未开始 | 成果、摘要、浏览器、终端、响应式布局 | 面板状态与预览回归 |
| 5. 注释与下一轮修改 | 未开始 | 成果内直接选区、锚点、批注暂存与组合发送 | 选区/多批注/失败恢复/权限测试 |
| 6. 全量回归与打包 | 未开始 | Windows 构建、安装、升级、真实任务验收 | 全测试、安装包哈希、冒烟记录 |

状态只允许使用：`未开始`、`进行中`、`已完成`、`阻塞`。

## 每轮开始协议

1. 阅读本文件和权威规格。
2. 执行 `git status --short`、`git branch --show-current`、`git log -5 --oneline`。
3. 确认正在远端 `E:\harmony-work` 对应的共享仓库工作，禁止切回废弃的 xiaoheng-workbuddy 项目。
4. 查看“阶段台账”和“最近检查点”，只推进当前阶段。
5. 若发现未提交变更，先识别来源并与其协作，不得回滚未知变更。
6. 开始编辑前写明本轮计划和受影响文件。

## 每轮结束协议

1. 更新阶段状态和最近检查点。
2. 记录实际修改文件、已执行测试、失败测试和未完成项。
3. 运行 `git diff --check`。
4. 只有验证通过后才能标记阶段完成。
5. 用独立提交保存可恢复检查点；不得把安装包和 `release/` 纳入 Git。
6. 用一句话写明下一轮的第一步，避免重复探索。

## 最近检查点

### 2026-09-15：计划勾选同步与任务 1-4 远端复验

- 实施计划的复选框首次与真实进度对齐：任务 1-4 共 26 个步骤已勾选，任务 5-12 保持未勾选，消除「已完成但显示未开始」的歧义。
- 未轻信台账既有结论，逐项在远端 Windows 复验阶段 0-1 与任务 4 的成果：品牌单测 1/1、品牌扫描 1/1、安装器离线运行时 8/8、Tauri IPC 24/24 通过。
- 修复任务 4 遗留的 TypeScript 编译错误：`src/services/tauri/tauri.test.ts` 使用了 `Array.prototype.at`，而 `tsconfig.json` 的 `lib` 为 ES2020，导致 `npm run build:web`（`tsc && vite build`）无法通过。改为下标访问后 `npx tsc --noEmit` 为 0 错误。
- 固化远端构建环境：非交互 SSH 会话没有 MSVC 环境变量，`cargo test` 会因找不到 `cl.exe` 而失败，且管道会吞掉非零退出码造成「假成功」。已在 `E:\xiaoheng-tools\winbuild.cmd` 固化 `vcvars64` + `CARGO_HOME`/`RUSTUP_HOME` 包装脚本，后续 Rust 验证统一经由该脚本执行。
- 未完成项：任务 5 的持久化代码已就位但仍需 Rust 侧 `store::db` 与 `codex_runtime::service` 测试确认后才可勾选。

### 2026-09-15：Codex 公开执行事件与脱敏

- 新增公开思考摘要开始/增量事件和命令输出增量事件，保持既有 `agent_event_<sessionId>` IPC 通道不变。
- `item/reasoning/summaryPartAdded` 与 `summaryTextDelta` 可进入前端；`item/reasoning/textDelta` 明确丢弃，不向 UI 暴露模型原始推理。
- 工具输入、审批参数、命令增量和工具结果统一执行递归敏感字段脱敏、ANSI/控制字符清理与 UTF-8 安全限长。
- 远端 Windows 验证：`codex_runtime::event_adapter` 8/8 通过；`src/services/tauri/tauri.test.ts` 23/23 通过；定向 `rustfmt --check` 通过。
- 全仓 `cargo fmt --check` 仍会报告既有未格式化文件，本检查点仅格式化本次修改的事件适配器，避免无关改动。

### 2026-09-15：Windows 发行品牌与版本

- 版本统一为 `1.3.0-alpha.1`，npm、Cargo、Tauri 与 WPF Bootstrapper 的产品名、可执行文件名和安装包名统一为 Harmony Work。
- Tauri identifier、旧安装目录、旧快捷方式与配置目录保留为升级兼容入口；新安装和运行均使用 `harmony-work.exe` 与 Harmony Work 品牌。
- Codex app-server 客户端身份更新为 Harmony Work，Agent 开发者指令明确限定自我名称为“小亨”。
- README、发行链接、安装器界面、托盘、窗口和运行日志中的用户可见旧品牌已完成迁移。
- 远端 Windows 验证：默认测试 74 项通过；Codex process 5/5、background 2/2、runner 5/5、Harvis 10/10、Artifact Runtime 5/5 通过；Web 生产构建成功；WPF Bootstrapper 构建 0 错误（保留 3 条既有 nullable 警告）。
- 旧名称扫描仅剩兼容常量、兼容数据路径、规格文档与迁移测试中的明确白名单项。

### 2026-09-15：品牌命名契约

- 新增 `src/branding.ts`，明确区分产品品牌 Harmony Work 与默认 Agent 名称小亨。
- 新增品牌单测，远端 Windows 执行 `npx vitest run src/branding.test.ts`，1/1 通过。
- 新增 `scripts/branding-contract.test.mjs`，首次扫描准确检出应用、安装器、README 和发行元数据中的旧产品品牌；在阶段 1 完成前保持为独立红灯测试。
- 默认 `npm test` 暂不接入品牌扫描，待旧产品文案迁移完毕后接入。

### 2026-09-15：应用内品牌与图标迁移

- 保存用户确认的 Harmony Work 文字标识和应用图标源文件，新增可重复执行的品牌资产生成脚本。
- 生成透明背景文字标识、512px Web 图标和完整 Tauri 桌面/移动图标组；`logo.png` 与 `piscis.png` 仅作为兼容 URL，内容已统一为新图标。
- 左侧栏、关于页、首次引导、环境引导、Overlay、IDE 欢迎页、对话初始页与营销工作台已改为 Harmony Work 产品品牌。
- 默认 Agent 名称、会话角色和“给小亨发送消息”语义保持为小亨。
- 远端 Windows 验证：品牌与设计测试 3/3 通过；`npm run build:web` 成功。
- 独立品牌扫描剩余项仅为 Tauri 元数据、WPF 安装器和 README，进入下一检查点处理。

### 2026-09-15：规格建立

- 已确认对话交互原型。
- 已确认产品品牌名为 Harmony Work。
- 已确认默认 Agent 自我认知名称继续为小亨。
- 已确认使用“对话内时间线 + 右侧工具面板”的组合结构。
- 已确认成果修改采用 Codex 风格直接注释：在成果上选择或框选目标，锚点浮层填写要求，再添加到主对话统一发送。
- 已确认右侧预览下方不设置常驻评论输入框。
- 已确认建立本台账支持多轮续接。
- 已完成逐文件、分测试检查点的实施计划：`docs/superpowers/plans/2026-09-15-harmony-work-conversation-experience.md`。
- 已将产品品牌、Agent 名称、公开思考摘要、结构化时间线、右侧工作台、成果内选区批注、Windows 打包和升级兼容纳入同一验收链路。
- 尚未修改生产代码。

## 当前下一步

阶段 2 进行中：持久化结构化执行历史与消息批注上下文，为时间线重建和成果修改闭环提供数据基础。

## 验证证据

当前已记录阶段 1 的 Windows 测试、构建和品牌扫描证据。后续每完成一个阶段，继续追加：

- 命令。
- 通过数量。
- 关键截图或产物路径。
- 对应提交哈希。

不要记录密码、API Key、Cookie、Token 或其他敏感信息。
