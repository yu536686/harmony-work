# 小亨 WorkBuddy 项目对话记录

> 本文件用于远端 Codex 接手和应用内项目历史展示。SSH 密码等敏感认证信息已移除，禁止重新写入源码或构建产物。

## 1. 参考项目

**用户：**

分析 `njbinbin-piscis/openpiscis` 项目。

**用户补充：**

`njbinbin-piscis/piscis-engine` 是 engine 项目，需要一并分析。

## 2. 产品开发需求

**用户：**

参考上述项目为亨吉利开发相似的 AI 桌面工作台。角色设计只作参考：主协调 Agent 命名为 `xiaoheng`，中文名“小亨”；Koi 改为 `manager`，UI 中文名“经理”；Fish 改为 `har`，中文名“职员”；鱼池改为“办公区”。其他产品能力参考原项目。

交付目标为可在 Windows 安装的 EXE。UI 参考用户提供的浅色数据后台图片，要求兼顾性能和兼容性，并提供流畅动画。主 Logo 使用 `/Volumes/SSD/Logo/PNG/纯logo.png`。工程和构建文件放在 `/Volumes/SSD/小亨workbuddy`，避免占用 Mac 内置存储。

## 3. 第一阶段实现结果

**Codex：**

选择 Tauri 2 + Rust + React/TypeScript/Vite 独立实现，原因是相较 Electron 具有更小的安装体积和运行内存，同时可利用 WebView2 提供现代 UI 和动画能力。

已完成：

- 小亨、经理、职员、办公区的角色与数据模型
- 仪表盘、AI 对话、办公区、团队、任务、记忆、技能、日程和设置页面
- OpenAI、Anthropic、OpenAI 兼容接口和 Ollama 模型接入
- 本地 JSON 原子持久化
- Windows Credential Manager / macOS Keychain 凭据保存
- 响应式布局和 Framer Motion 动效
- Windows NSIS EXE / MSI 构建配置和 GitHub Actions
- 使用指定 Logo 生成 Windows ICO 和应用图标

前端生产构建、Rust `cargo check`、格式检查和 Clippy 均通过；在 Mac 上生成并检查了测试应用。界面导航、聊天预览、任务创建和响应式布局已完成浏览器验收。

当前明确边界：这是可运行的产品 MVP，并非 OpenPiscis 的全功能等价实现。MCP、即时通讯、浏览器/Shell 工具、后台日程执行和常驻自主多 Agent 运行时仍需后续开发。

## 4. Windows 构建需求

**用户：**

局域网 `192.168.11.5` 有一台开启 SSH 的 Windows PC，并安装了 Codex。要求把完整项目和开发逻辑传递给远端，配置本地部署环境并编译最终 EXE。

**Codex：**

已准备 `CODEX_HANDOFF.md` 和 `scripts/bootstrap-windows.ps1`。脚本负责检查或安装 Node.js、Rust stable MSVC、Visual Studio C++ Build Tools，随后执行 TypeScript/Vite 构建、Rust 格式与 Clippy 检查，以及 NSIS/MSI 安装包构建。

## 5. 对话交接要求

**用户：**

要求把整个项目对话复制到 Windows，并能在应用界面显示。

**Codex 执行约束：**

- 本文档随项目传输给远端 Codex。
- 项目关键对话写入 `src/defaultState.ts`，Windows 首次启动后在“与小亨沟通”页面显示。
- SSH 密码已从本交接记录移除，不得提交或持久化。

