# Office Artifact Runtime 设计

## 目标

为小亨提供跨平台、可重复、低轮次的 DOCX、PPTX、XLSX 生成能力。新文件默认走结构化 Artifact 工具，不再依赖 Microsoft Office COM，也不要求模型临时编写长脚本。现有 COM 能力保留为兼容层，只处理宏、复杂已有文件和必须由本机 Office 精确渲染的任务。

## 设计原则

1. **结构化输入优先**：模型只提交文档内容和版式意图，运行时负责确定性实现。
2. **一次生成、一次验证**：生成器在同一调用内创建文件并检查 OOXML 包结构，避免“写脚本、读脚本、执行、反复核对”的高 Token 链路。
3. **生成与渲染分离**：DOCX/PPTX/XLSX 生成不依赖 Office；视觉检查由 `artifact_render` 调用 LibreOffice headless 完成。
4. **兼容回退可见但不抢占**：`office` 继续注册为 Windows 兼容工具，但不进入默认 Office 工具组和用户工具目录。
5. **失败必须明确**：子进程非零退出、输出缺失、OOXML 结构不完整均返回失败，不允许以部分 stdout 误报成功。

## 架构

```text
Agent structured call
  -> document_artifact / presentation_artifact / spreadsheet_artifact
  -> Rust Artifact Runtime bridge
  -> bundled Node.js CLI
  -> docx / PptxGenJS / ExcelJS
  -> OOXML package verification
  -> ToolResult + artifact metadata

Optional visual QA
  -> artifact_render
  -> LibreOffice headless
  -> PDF (and PNG when a PDF rasterizer is available)

Compatibility-only path
  -> office (Windows COM)
```

## 运行时协议

Rust 将结构化输入写入工作区 `.workbuddy/artifact-runtime/jobs/<uuid>.json`，然后执行：

```text
node artifact-runtime.cjs <document|presentation|spreadsheet> <spec.json>
```

CLI 仅向 stdout 输出一条 JSON 结果：

```json
{
  "ok": true,
  "kind": "document",
  "path": "C:\\Users\\User\\Desktop\\report.docx",
  "bytes": 18342,
  "details": { "sections": 4, "packageEntries": 17 }
}
```

错误写入 stderr 并以非零状态退出。Rust 始终同时检查退出状态、JSON `ok`、文件存在性、文件大小和扩展名。

## 工具接口

### `document_artifact`

创建新的 `.docx`。输入包含 `path`、标题、作者、页眉页脚及 `sections`。章节可包含段落、项目符号、表格、图片和分页。第一阶段不修改已有复杂文档；此类任务由 `office` 兼容层处理。

### `presentation_artifact`

创建新的宽屏 `.pptx`。输入包含 `path`、主题、封面信息和 `slides`。运行时提供固定的封面、章节、要点、双栏、图片和总结布局，避免模型生成任意坐标脚本。

### `spreadsheet_artifact`

创建新的 `.xlsx`。输入包含 `path` 与 `sheets`。工作表支持列定义、行数据、单元格公式、冻结表头、筛选和基础格式。宏、数据透视表、外部数据连接和复杂图表编辑由 `office` 兼容层处理。

### `artifact_render`

将 Office Artifact 转换为 PDF，或在可用时进一步栅格化为 PNG。工具自动寻找常见 LibreOffice 安装路径并使用独立用户配置目录，避免锁住用户正在打开的 Office 文档。

## 工具路由

`office` 意图组默认暴露：

```text
document_artifact
presentation_artifact
spreadsheet_artifact
artifact_render
pdf
```

旧 `presentation`、`office` 和 `com_invoke` 不进入默认组。系统提示明确要求：新建 Office 文件先调用 Artifact 工具；只有遇到明确不支持的已有文件、宏或原生 Office 精确操作时，才动态发现并调用兼容工具。

## 依赖与打包

- `docx`：DOCX 生成，MIT。
- `pptxgenjs`：PPTX 生成，MIT。
- `exceljs`：XLSX 生成，MIT。
- `jszip`：OOXML 包检查，MIT/GPL 双许可，按 MIT 使用。
- `esbuild`：将运行时打包为单个 CommonJS 文件。
- LibreOffice：外部可选运行时，仅用于渲染和转换，不嵌入安装包。

构建脚本在 Web/Tauri 构建前生成 `src-tauri/resources/artifact-runtime/artifact-runtime.cjs`。Rust 使用 `include_bytes!` 将该文件嵌入主程序，因此安装后不依赖 `node_modules`。目标电脑仍需 Node.js；现有初始化器负责检测和安装。第三方许可证随安装资源分发。

## 安全与权限

- 输出路径支持绝对路径或相对当前工作区路径。
- 创建文件属于写操作，沿用会话权限确认策略。
- 临时规格只写入当前工作区 `.workbuddy`，执行完成后删除。
- CLI 不接受任意 JavaScript，只接受经过 schema 限制的 JSON。
- 图片路径只作为本地素材读取，不支持 URL 自动下载。

## 错误与回退

1. 找不到 Node：返回初始化器安装 Node.js 的明确提示。
2. 生成失败：返回子进程状态与截断后的 stderr，不声称文件已完成。
3. OOXML 包不完整：删除无效产物并返回验证失败。
4. 找不到 LibreOffice：保留原 Artifact，提示渲染依赖缺失；不回退到 COM。
5. 需求超出结构化能力：模型说明限制后改用 `office`，不得循环重复同一 Artifact 调用。

## 测试策略

- Node 单元/集成测试分别生成 DOCX、PPTX、XLSX，检查 ZIP 必要条目、数量和文件大小。
- Rust 测试检查工具 schema、扩展名验证、默认 Office 工具路由以及 COM 隐藏策略。
- 本地集成测试调用打包后的 CLI，生成三种样例文件并解析验证。
- Windows 打包后检查安装包版本、文件大小、SHA-256，并在远端执行三类生成 smoke test。

