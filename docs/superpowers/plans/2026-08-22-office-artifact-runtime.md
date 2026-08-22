# Office Artifact Runtime 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 用确定性的跨平台 Artifact Runtime 取代 COM 作为 Word、PowerPoint、Excel 新文件生成的默认路径，并产出 0.9.4 Windows 安装包。

**架构：** 四个结构化 Rust 工具通过统一桥接器调用单文件 Node.js 运行时；运行时分别使用 docx、PptxGenJS、ExcelJS 生成 OOXML，并在返回前验证包结构。LibreOffice 仅负责可选渲染，Windows COM 保留为隐藏兼容层。

**技术栈：** Rust、Piscis Tool API、TypeScript、Node.js、docx、PptxGenJS、ExcelJS、JSZip、esbuild、LibreOffice headless、Vitest。

---

## 文件结构

- 创建 `artifact-runtime/src/cli.ts`：单文件运行时入口和结果协议。
- 创建 `artifact-runtime/src/document.ts`：DOCX 结构化生成。
- 创建 `artifact-runtime/src/presentation.ts`：PPTX 固定布局生成。
- 创建 `artifact-runtime/src/spreadsheet.ts`：XLSX 结构化生成。
- 创建 `artifact-runtime/src/verify.ts`：OOXML ZIP 结构验证。
- 创建 `artifact-runtime/src/artifact-runtime.test.ts`：三种格式的红绿集成测试。
- 创建 `scripts/build-artifact-runtime.mjs`：esbuild 单文件打包。
- 创建 `src-tauri/src/tools/office_artifacts.rs`：四个 Tool 和统一 Node/LibreOffice 桥接。
- 修改 `src-tauri/src/tools/mod.rs`：导出跨平台 Artifact 工具。
- 修改 `src-tauri/src/host.rs`：注册四个工具。
- 修改 `src-tauri/src/commands/chat.rs`：更新 Office 工具组和系统提示。
- 修改 `src-tauri/src/commands/config/tools.rs`：显示新工具并隐藏 legacy COM。
- 修改 `package.json`、`package-lock.json`：依赖、构建命令和 0.9.4 版本。
- 修改 `src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.lock`：0.9.4 版本和资源。
- 创建 `src-tauri/resources/artifact-runtime/THIRD_PARTY_NOTICES.txt`：依赖许可证声明。

### 任务 1：锁定运行时行为

- [ ] 在 `artifact-runtime/src/artifact-runtime.test.ts` 编写三个失败测试：分别生成 `.docx`、`.pptx`、`.xlsx`，断言输出存在、大小大于 1 KB、ZIP 中包含格式必要条目。
- [ ] 运行 `npm test -- artifact-runtime/src/artifact-runtime.test.ts`，确认因模块不存在而失败。
- [ ] 安装 `docx pptxgenjs exceljs jszip` 和直接开发依赖 `esbuild`。

### 任务 2：实现三个确定性生成器

- [ ] 实现 `document.ts`，支持标题、段落、项目符号、表格、图片和分页。
- [ ] 运行 DOCX 测试，确认由红转绿。
- [ ] 实现 `presentation.ts`，支持封面和 section/bullets/two_column/quote/image/summary 固定布局。
- [ ] 运行 PPTX 测试，确认由红转绿。
- [ ] 实现 `spreadsheet.ts`，支持列、行、公式、冻结和筛选。
- [ ] 运行 XLSX 测试，确认由红转绿。
- [ ] 实现 `verify.ts` 和 `cli.ts`，统一输出 JSON 结果并在结构不完整时非零退出。

### 任务 3：构建可嵌入运行时

- [ ] 编写 `scripts/build-artifact-runtime.mjs`，将 CLI 打包到 `src-tauri/resources/artifact-runtime/artifact-runtime.cjs`。
- [ ] 修改 `package.json`，确保 `npm run build` 和 Tauri 构建前先执行 `build:artifact-runtime`。
- [ ] 运行打包 CLI 生成三种样例并再次检查 OOXML。

### 任务 4：Rust 工具接入

- [ ] 在 `office_artifacts.rs` 先编写失败测试，覆盖扩展名校验、schema 必填字段、运行时结果解析和 LibreOffice 查找候选。
- [ ] 实现 `ArtifactRuntime`：嵌入 CJS、定位 Node、写临时 spec、严格处理进程退出、验证输出并清理临时文件。
- [ ] 实现 `DocumentArtifactTool`、`PresentationArtifactTool`、`SpreadsheetArtifactTool` 和 `ArtifactRenderTool`。
- [ ] 在 `mod.rs` 和 `host.rs` 注册四个跨平台工具，运行 Rust 定向测试。

### 任务 5：默认路由和兼容策略

- [ ] 先添加失败测试，断言 `office` 组包含四个 Artifact 工具且不包含 `office`、`presentation`、`com_invoke`。
- [ ] 修改 `chat.rs` Office 工具组及系统提示，默认使用 Artifact，COM 仅在宏、复杂已有文件或原生 Office 精确操作时回退。
- [ ] 修改工具目录，显示四个 Artifact 工具并隐藏 legacy `office`。
- [ ] 运行相关 Rust 测试和前端工具目录测试。

### 任务 6：版本、许可证和完整验证

- [ ] 将 npm、Cargo、Tauri 版本统一更新为 `0.9.4`。
- [ ] 添加第三方许可证通知，并让 Tauri 安装包携带该资源。
- [ ] 运行 `npm test`、`npm run build`、`cargo test --manifest-path src-tauri/Cargo.toml`、`git diff --check`。
- [ ] 生成三种真实样例 Artifact，验证文件结构；若本机有 LibreOffice，再验证 PDF 渲染。
- [ ] 检查变更范围后提交 `codex/0.9.4-office-artifacts` 分支。

### 任务 7：Windows 编译与交付

- [ ] 将提交同步到 `192.168.11.5` 的独立 Windows 工作树 `E:\xiaoheng-workbuddy-office-artifacts-0.9.4`。
- [ ] 在 Visual Studio Build Tools 环境运行 `npm ci`、`npm test` 和 `npm run tauri build`。
- [ ] 将 NSIS 安装包复制到工作树 `release` 目录，记录文件大小和 SHA-256。
- [ ] 在 Windows 运行打包后的 Artifact CLI smoke test，确认 DOCX、PPTX、XLSX 均可生成。

