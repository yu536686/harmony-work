# Harmony Work 成果审阅与发布 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将右侧成果区升级为可扩展的输出列表与按需预览页，并支持在成果和浏览器预览的可见区域直接框选批注，随后制作采用 Harmony Work 新品牌形象的完整 Windows 安装包。

**架构：** 成果视图保留现有工作台状态与 Tauri 本地预览协议，但把所有成果从横向 chip 列表改为垂直输出内容列表；只有用户打开的成果才进入预览标签。图片、HTML、PDF 和浏览器预览均在 iframe/画布上方叠加同一类坐标捕获层，提交的注释继续复用既有 `WorkbenchAnnotation` 与下一轮 Agent 注入流程。跨域网页和浏览器内置 PDF 不读取内部 DOM，批注坐标对应当前可见预览区域。

**技术栈：** React 18、TypeScript、Vitest/Testing Library、Tauri 2、Lucide React、CSS、WPF .NET 8、NSIS。

---

## 文件职责

- 修改：`src/components/Chat/WorkbenchPanel.tsx`：输出列表、打开成果预览页、浏览器可视区域批注。
- 新建：`src/components/Chat/BrowserAnnotationCanvas.tsx`：浏览器 iframe 上的框选、评论和暂存注释组件。
- 修改：`src/components/Chat/AnnotationLayer.tsx`：允许 PDF 预览进入可见区域框选。
- 修改：`src/components/Chat/ArtifactPreview.tsx`：将 PDF 预览接入批注层。
- 修改：`src/components/Chat/ConversationExperience.css`：输出列表、预览标签、批注画布和窄面板响应式样式。
- 修改：`src/components/Chat/WorkbenchPanel.test.tsx`：验证输出列表、按需打开预览和浏览器开始批注入口。
- 新建：`src/components/Chat/BrowserAnnotationCanvas.test.tsx`：验证框选后提交携带浏览器区域坐标与评论。
- 修改：`scripts/conversation-layout-contract.test.mjs`：移除横向 chip 约束，锁定列表、预览标签和面板响应式约束。
- 修改：`src/theme.css`、`src/themes/gold.css`：Windows 优先的雅黑 UI 字体栈，macOS 回退苹方。
- 修改：`installer/WorkBuddyBootstrapper/MainWindow.xaml`、`installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj`：Harmony Work 紫色品牌安装界面和品牌资源。
- 修改：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`scripts/offline-runtime-lock.test.mjs`：发布版本更新为 `1.3.0-alpha.2`。

### 任务 1：成果列表与按需预览

- [x] 编写失败的工作台测试：两个成果在“输出内容”列表出现，初始仅显示第一个预览，点击第二项后在预览标签中打开第二个成果。
- [x] 运行静态与工作台测试，确认新断言因现有横向列表失败。
- [x] 将 `selectedArtifactId` 拆为“当前预览成果”和已打开成果 ID 列表；列表按钮负责打开，预览标签只渲染已打开项目且保持名称截断。
- [x] 将 `.workbench-artifact-list` 改为纵向输出列表，新增预览标签样式；保留系统打开、另存为和文件路径动作。
- [x] 在远端 Windows 运行工作台测试，确认通过。

### 任务 2：直接框选批注

- [x] 编写失败的工作台区域批注测试：开启模式后拖拽选择区域、填写意见、提交后产生 `target.type === "browser"` 与 `selection.type === "image_region"` 的暂存注释。
- [x] 运行测试确认旧实现缺少区域批注画布。
- [x] 实现浏览器批注画布：在 iframe 外包裹定位容器，开启时覆盖透明捕获层，框选后显示评论浮层，取消不产生注释。
- [x] 将 `WorkbenchPanel` 的“批注页面”改为“开始注释”开关并接入浏览器画布；对于历史浏览步骤保留页面级备注作为无 iframe 的降级模式。
- [x] 扩展 `AnnotationLayer` 的预览类型与捕获条件，允许 `pdf`；`ArtifactPreview` 直接传入 PDF 类型并开启图层。
- [x] 在远端 Windows 重新运行组件测试，确认通过。

### 任务 3：字体与安装器形象

- [x] 更新静态契约，锁定成果列表、预览标签和 `1.3.0-alpha.2` 发布元数据。
- [x] 运行静态契约，确认旧横向成果实现与旧版本号失败。
- [x] 更新全局与黑金主题字体变量，代码字体继续使用等宽字体。
- [x] 重构 WPF 安装主窗口为 Harmony Work 的白底、淡紫表面和紫色操作层级；保持安装路径、自定义选项、进度条和现有控件 `x:Name` 不变。
- [x] 更新 WPF 资源为 Harmony Work 图标，确保外层安装器不再复用旧 WorkBuddy 品牌位图。
- [x] 再次运行静态契约断言，确认通过。

### 任务 4：版本、回归与完整安装包

- [x] 先更新离线运行时锁定测试的期望版本为 `1.3.0-alpha.2` 并运行，确认在版本文件仍为 alpha.1 时失败。
- [x] 同步更新 npm、Tauri 和 Cargo 的版本来源，运行锁定测试确认通过。
- [x] 在远端 Windows 运行 `npm test`、`npm run build`、Tauri release build 和 `git diff --check`。
- [x] 远端 Windows 使用隔离 Cargo target 构建新的 Tauri NSIS 内层安装器，再用 `scripts/build-full-installer.ps1` 打包含运行环境的 WPF 外层安装器。
- [x] 校验外层安装器文件存在、读取版本命名和 SHA-256；未实际双击验收 GUI 安装流程。

## 规格覆盖检查

- 成果过多的可见性：任务 1 用垂直输出列表取代横向成果 chip，并仅为已打开成果创建预览标签。
- 成果与浏览器直接批注：任务 2 支持框选可见预览区域；下方通用对话框不是主要入口。
- 字体：任务 3 用 Windows 原生微软雅黑 UI 优先、苹方回退，覆盖全局及黑金主题。
- 新产品形象安装器：任务 3 只改视觉资源与呈现，不更改已验证的载荷、进度与环境检测逻辑。
- 发布安装包：任务 4 在远端 Windows 编译内层 NSIS 与外层 WPF Bootstrapper。
