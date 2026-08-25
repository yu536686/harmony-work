# 小亨 WorkBuddy

Harmony AI 桌面工作台。小亨（xiaoheng）是主协调 Agent，经理（manager）负责长期领域与任务拆解，职员（har）负责清晰、有限的执行任务，项目承载协作。

## 已实现

- 小亨 AI 对话由 Codex app-server 驱动，当前支持 OpenAI 与 DeepSeek Responses API
- API Key 使用 Windows Credential Manager / macOS Keychain 保存
- 办公区、经理、职员、看板任务、长期记忆、技能和自动日程界面
- 本地状态持久化与原子写入
- 响应式桌面布局、页面切换、卡片、图表和状态动效
- Tauri 最小前端权限与 CSP
- Windows NSIS `.exe` 和 MSI 原生 CI 构建

## 运行架构

- 主会话使用 Codex app-server，并适配现有消息、Todo、活动、审批与成果协议。
- IM、定时任务、CLI 和无头任务使用 `codex exec`。
- Harvis、Office、技能、记忆、IM 与 StaffDeck 通过 WorkBuddy MCP Host 提供。
- IM 渠道仅启用企业微信和个人微信 iLink。

## Mac 开发环境

所有工具链和构建缓存均位于外置 SSD：

```zsh
source scripts/env-mac.sh
npm install
npm run tauri dev
```

或直接运行：

```zsh
./scripts/dev-mac.sh
```

## Windows 安装包

Windows 安装包必须在 Windows 环境构建。将项目提交到 GitHub 后，手动运行 `Build Windows Installer` workflow，产物中会包含：

- `小亨 WorkBuddy_*_x64-setup.exe`
- `小亨 WorkBuddy_*_x64_en-US.msi`

也可以在 Windows 11 管理员 PowerShell 中运行 `scripts/bootstrap-windows.ps1`，自动安装缺失的 Node.js、Rust 和 Visual Studio C++ Build Tools，然后执行检查和构建。开发与后续 Codex 接手说明见 `CODEX_HANDOFF.md`。

## 数据位置

业务状态使用 Tauri 应用数据目录保存；API Key 不进入状态文件。模型请求会发送到用户在设置中选择的模型服务。
