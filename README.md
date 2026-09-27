<div align="center">

<img src="src-tauri/icons/icon.png" width="128" height="128" alt="Harmony Work Logo">

# Harmony Work

**让 AI 不只回答问题，还能在项目、文档、创作、工具与 IM 会话中完成工作。**

[![Version](https://img.shields.io/badge/version-1.0.6-7654E8?style=flat-square)](https://www.harmonywork.com.cn/)
[![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D4?style=flat-square&logo=windows11&logoColor=white)](#windows-安装)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-18-149ECA?style=flat-square&logo=react&logoColor=white)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-stable-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Codex](https://img.shields.io/badge/Agent-Codex%20Harness-111111?style=flat-square)](https://github.com/openai/codex)

[下载 Windows 版](https://www.harmonywork.com.cn/downloads/workbuddy/windows) · [功能概览](#功能概览) · [安装与配置](#windows-安装) · [源码构建](#从源码构建) · [常见问题](#常见问题)

</div>

---

## 项目简介

Harmony Work 是一款面向 Windows 的桌面 AI Agent 工作台。它将对话、项目、任务、文件、长期记忆、技能、创作工具、办公文档和即时通讯整合到同一套工作流中，让模型能够理解目标、调用工具、请求批准、持续执行并交付可下载的成果。

主会话由 **Codex app-server** 驱动，无人值守任务通过 **Codex exec** 执行；Harvis、Office、技能、记忆、IM、StaffDeck 等业务能力由独立的 **WorkBuddy MCP Host** 提供。界面继续负责项目与会话管理、Todo、活动记录、权限审批和成果展示。

产品名为 **Harmony Work**，对话中的助手仍称为 **小亨**。首次使用时，请先设置模型、工作区和文件访问权限；授权范围会影响 Agent 能访问的本机资源。

## 功能概览

| 能力 | 说明 |
| --- | --- |
| **Agent 对话工作台** | 支持流式回复、附件、模型切换、Todo、工具活动、权限审批、成果卡片和任务中断。 |
| **项目与会话** | 按项目目录组织会话，保存上下文、工作区、文件引用和 Codex Thread 映射。 |
| **StaffDeck 多 Agent 协作** | 创建不同职责的人员，分派任务，在看板中跟踪执行状态和交付结果。 |
| **Harvis AI 创作** | 提供图片与音乐创作页面，支持参考图、提示词助手、任务进度、结果预览与下载。 |
| **Office 成果生成** | 通过 Artifact Runtime 创建并验证 Word、PowerPoint 和 Excel 文件，生成后直接进入成果区。 |
| **技能与 MCP** | 浏览多个技能市场渠道，查看已安装版本、下载与安装进度，并可更新、打开目录或卸载技能。 |
| **长期记忆** | 保存用户偏好、项目事实和执行经验，并通过关键词与向量检索按需注入上下文。 |
| **素材库与数据看板** | 登录后浏览、筛选和预览素材，将文件暂存到会话，或基于看板数据新建分析任务。 |
| **个人设置云同步** | 登录后可按类别选择恢复或加密备份模型、提供商、API Key 与通用偏好；云端只保存端到端加密密文，本机路径和 IM 凭据不迁移。 |
| **IM 渠道** | 支持企业微信智能机器人长连接和个人微信 iLink，将消息转为可持续执行的 Agent 会话。 |
| **自动化与桌面工具** | 支持定时任务、文件操作、Shell、浏览器控制和 Windows UI 自动化。 |

## 运行架构

```mermaid
flowchart LR
    UI[React / Tauri Desktop UI]
    HOST[WorkBuddy Application Host]
    APP[Codex app-server]
    EXEC[Codex exec]
    MCP[WorkBuddy MCP Host]
    TOOLS[Harvis / Office / Skills / Memory]
    BIZ[IM / StaffDeck / Marketing / Automation]

    UI -->|Tauri IPC| HOST
    HOST -->|交互式会话| APP
    HOST -->|IM、定时与后台任务| EXEC
    APP -->|MCP| MCP
    EXEC -->|MCP| MCP
    MCP --> TOOLS
    MCP --> BIZ
```

- **Codex app-server**：管理交互式线程、上下文、计划、工具调用、审批和增量事件。
- **Codex exec**：运行 IM、定时任务和其他无头任务。
- **WorkBuddy Application Host**：管理桌面窗口、用户、项目、会话索引、权限 UI 和成果索引。
- **WorkBuddy MCP Host**：向 Agent 提供业务工具，不维护第二套 Agent Loop。

## Windows 安装

### 系统要求

- Windows 10 或 Windows 11 x64，推荐使用最新 Windows 11。
- 通过全量安装包安装时，离线 WebView2 Runtime 已内置，无需另行下载。
- 可访问所选模型服务的网络，以及对应供应商的 API Key。
- 若使用 Office 桌面控制能力，需要安装 Microsoft Office。
- 若使用企业微信企业能力，安装包已包含 WeCom CLI，但仍需用当前 Windows 账户完成官方授权。

### 全量离线安装包

当前 Windows x64 全量安装包为 `Harmony-Work-1.0.6-x64-setup.exe`。它使用 WPF 安装界面，内置 Tauri 应用、离线 WebView2、Node.js、Python 嵌入式运行时、PortableGit、WeCom CLI、浏览器与桌面自动化组件，以及 Harmony Cut 的 Python 依赖。私有运行时不会覆盖系统环境；Python 无须通过系统 MSI 安装。企业微信授权、账号登录和模型 API Key 仍需由用户自行配置。

### 1. 下载安装包

从 [Harmony Work 官网](https://www.harmonywork.com.cn/) 下载 Windows x64 安装包。官网入口会跳转至腾讯云 CDN：

```text
Harmony-Work-<version>-x64-setup.exe
```

需要核对文件完整性时，在安装包所在目录运行：

```powershell
Get-FileHash .\Harmony-Work-<version>-x64-setup.exe -Algorithm SHA256
```

1.0.6 安装包的 SHA-256 为 `89725fc46ba8f288925e1fc5082ac4c3079b3bb0d335f049bc914953998023ca`。其他版本应以官网发布信息中的摘要为准。

### 2. 运行安装程序

双击安装包并按提示完成安装。安装包可能尚未包含受信任的 Windows 代码签名，因此 SmartScreen 或智能应用控制可能显示“未知发布者”。请只从官网获取安装包并核对 SHA-256；受企业策略限制的设备应由管理员核准，不建议关闭系统安全防护。

### 3. 运行环境

使用全量离线安装包时，安装阶段已经部署以下私有运行时：

1. Node.js
2. Python 与 Harmony Cut 依赖
3. PortableGit
4. WeCom CLI

运行时位于 `%LOCALAPPDATA%\Harmony\WorkBuddy\runtime`，不会修改系统 PATH，也不会覆盖电脑上已有的开发环境。更新时会检查已安装组件，仅补装缺失或版本不匹配的载荷。

### 4. 登录或跳过

- **登录**：解锁线上营销工作台、素材库、数据看板、技能市场和团队协作等高阶模块。
- **注册**：打开线上注册页面。新账号需要管理员审批后才能登录高阶模块。
- **跳过**：直接进入基础对话功能，之后可以从左下角账户菜单再次登录。

首次成功登录后，若账号已有个人设置备份，应用会询问是否恢复；没有备份时可选择加密备份当前设置。之后可在“设置 → 我的云端配置”中按类别选择 AI 提供商与模型、视觉模型、Agent 运行偏好、工具偏好或交互与安全策略进行上传与恢复。工作区、会话、MCP、SSH 与 IM 渠道凭据始终保留在当前设备。

### 5. 配置模型

首次配置页面提供供应商指引、API Key 输入、模型列表获取、默认模型和启用模型选择。

当前 Codex 主执行链路重点验证：

- OpenAI Responses API
- DeepSeek Responses API
- Harmony 内部网关（Responses）

设置界面还提供 Anthropic、通义千问、MiniMax、智谱 GLM、Kimi、SiliconFlow、Harmony 和自定义 OpenAI 兼容接口。实际可用性取决于供应商是否兼容当前 Codex Responses 执行协议。

**Harmony 内部网关（公司统一计费）**：选择「Harmony-亨吉利」后，Base URL 默认为 `https://api.harmonywatch.cn/v1`，填入门户「我的控制台」中生成的 Harmony API Key 即可调用。用量由网关按个人 Key 统计。

门户入口为 `https://portal.harmonywatch.cn`，技能市场为 `https://market.harmonywatch.cn`。升级时会自动迁移已知旧公网端口地址（8090、8787、13001）；局域网与自定义服务地址保持不变，无需重新填写 API Key。

推荐配置流程：

1. 选择 AI 供应商。
2. 按页面链接创建并填写 API Key。
3. 点击“获取模型列表”。
4. 选择启动/默认模型。
5. 勾选允许在会话底部切换的模型。
6. 选择工作区目录和会话权限策略。

API Key 和 IM 密钥在写入本地设置前使用 ChaCha20-Poly1305 加密。模型请求会发送到用户选择的模型服务，请根据业务的数据安全要求配置供应商和权限模式。

## 常用配置

### 会话权限

小亨提供不同的执行权限级别。需要修改文件、运行 Shell 或控制桌面时，Agent 会遵循当前会话权限并在必要时弹出批准请求。建议日常使用“请求批准”，只在可信任务中开启“完全访问”。

### 企业微信

1. 在企业微信中创建智能机器人，选择 API 模式并启用长连接。
2. 在“设置 → IM 渠道 → 企业微信”填写 Bot ID 和 Secret。
3. 保存后点击“连接渠道”。
4. 如需通讯录、消息、文档、会议、日程和待办能力，再执行：

```powershell
%LOCALAPPDATA%\Harmony\WorkBuddy\runtime\wecom\wecom-cli.cmd auth init
```

旧版 CLI 使用：

```powershell
%LOCALAPPDATA%\Harmony\WorkBuddy\runtime\wecom\wecom-cli.cmd init
```

### 个人微信 iLink

1. 在“设置 → IM 渠道 → 微信”启用通道。
2. 保存设置并点击“连接渠道”。
3. 点击“绑定微信”，使用手机微信扫描二维码并确认授权。

绑定完成后，微信消息会关联到 WorkBuddy 的 IM 项目会话。

## 从源码构建

### 获取源码

仓库当前为私有项目，拉取前需要具有访问权限并完成 GitHub 登录：

```powershell
git clone https://github.com/yu536686/harmony-work.git
cd harmony-work
git switch main
```

### 一键准备并构建

在 Windows 11 管理员 PowerShell 中运行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\bootstrap-windows.ps1
```

脚本会检查 Node.js、Rust MSVC 工具链和 Visual Studio C++ Build Tools，并准备构建依赖。开发环境的安装与产品全量离线安装包是两条不同流程。

### 已有开发环境

环境要求：

- Git
- Node.js 22 或更高版本
- Rust stable MSVC toolchain
- Visual Studio 2022 Build Tools，包含“使用 C++ 的桌面开发”工作负载

执行：

```powershell
npm ci
npm test
npm run build
cargo test --manifest-path src-tauri\Cargo.toml --workspace
npm run tauri build -- --target x86_64-pc-windows-msvc --bundles nsis,msi
```

Tauri 内层安装包位于：

```text
target\x86_64-pc-windows-msvc\release\bundle\nsis\
target\x86_64-pc-windows-msvc\release\bundle\msi\
```

## 全量安装包构建

在 Windows 构建机上、完成依赖安装后执行：

```powershell
.\scripts\build-full-installer.ps1
```

脚本校验离线载荷、构建 Tauri NSIS 内层安装包，再生成 WPF 外层安装器。最终文件位于 `release\full\Harmony-Work-<version>-x64-setup.exe`。构建与安装细节见 [安装器说明](installer/README.md)。运行时载荷和打包产物不提交到 Git。

## macOS 开发预览

macOS 版本正在同一仓库的功能分支开发，暂未提供签名、Apple 公证或官网正式下载。当前只验证了 Apple Silicon 构建；Intel Mac 和完整功能回归尚未完成。最低系统版本为 macOS 13。

构建机需要 Node.js `v24.18.0`、Rust stable、Xcode Command Line Tools，以及可用的网络来安装锁定的 npm/Cargo 依赖。不要在 Windows 共用的 `node_modules` 中直接执行 macOS 的 `npm ci`；先使用独立的 Mac 检出目录。

```sh
npm ci
npm test
./scripts/build-mac.sh
```

脚本使用 Mac 专属 Tauri 配置，不执行 Windows FlaUI/安装器暂存。生成的 `.app` 和 `.dmg` 位于脚本输出的 Cargo target 目录。桌面操控首次使用时仍需用户在“系统设置 → 隐私与安全性”中授予辅助功能和屏幕录制权限；安装包不能代为授权。

## 项目结构

```text
harmony-work/
├─ src/                              React 桌面界面与状态管理
├─ src-tauri/                        Tauri / Rust 应用宿主
│  └─ workbuddy-mcp-host/            WorkBuddy 业务 MCP Host
├─ artifact-runtime/                 Word、PowerPoint、Excel 成果运行时
├─ installer/                        Windows 全量离线安装器与运行时版本锁
├─ scripts/                          环境准备、Sidecar 暂存与构建脚本
└─ docs/                             设计与实施记录
```

## 开发验证

提交前至少执行：

```powershell
npm test
npm run build:web
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo clippy --manifest-path src-tauri\Cargo.toml --workspace -- -D warnings
cargo test --manifest-path src-tauri\Cargo.toml --workspace
```

涉及 Codex Sidecar、MCP Host 或 Office Artifact Runtime 时，还应执行：

```powershell
npm run test:codex-sdk
npm run test:codex-sidecar
npm run test:mcp-host
npm run test:artifact-runtime
```

## 常见问题

<details>
<summary><strong>安装程序被 Windows 标记为未知发布者</strong></summary>

安装包可能尚未完成 Windows 代码签名。请确认文件来自官网并核对 SHA-256。若设备启用了禁止未知应用的企业安全策略，请由管理员完成软件白名单或使用已签名的内部发行包，不建议关闭系统级安全保护。

</details>

<details>
<summary><strong>无法获取模型列表或发送消息</strong></summary>

检查 API Key、Base URL、模型 ID 和网络代理。主 Agent 需要兼容 Responses API 的模型服务；自定义兼容接口若只支持 Chat Completions，可能无法用于当前 Codex 主执行链路。

</details>

<details>
<summary><strong>WeCom CLI 已初始化，但应用仍显示未授权</strong></summary>

在安装应用的同一 Windows 账户下通过内置 WeCom CLI 完成 `auth init`，确认命令返回已授权状态，然后重启 Harmony Work 并重新测试。旧版 CLI 才使用 `wecom-cli init`。

</details>

<details>
<summary><strong>首次启动无法通过环境检测</strong></summary>

完成安装后先点击“重新检测”。如私有运行时缺失，请重新运行官网下载的全量安装包；它会检查并补装缺失组件，无须手动修改系统 `PATH`。

</details>

<details>
<summary><strong>构建时提示缺少 Codex 或 WorkBuddy MCP Host 资源</strong></summary>

先执行 `npm ci` 和 `npm run build`。构建脚本会从固定版本的 `@openai/codex-sdk` 暂存 Codex Sidecar，并编译、暂存 WorkBuddy MCP Host。

</details>

## 版本与反馈

- 当前版本：`1.0.6`，从 [官网](https://www.harmonywork.com.cn/) 下载。
- 问题反馈：[GitHub Issues](https://github.com/yu536686/harmony-work/issues)

---

<div align="center">

**Harmony Work · Codex-powered desktop AI work assistant**

</div>
