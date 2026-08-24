# 小亨 WorkBuddy — Windows Codex 开发交接

## 最终目标

为亨吉利交付 Windows 11 原生安装包。产品参考 OpenPiscis / piscis-engine 的产品思路，但采用独立实现，避免直接复制 BSL 授权源码。

角色映射固定如下：

- 主协调 Agent：`xiaoheng`，中文名“小亨”
- 原 Koi：`manager`，UI 中文名“经理”
- 原 Fish：`har`，UI 中文名“职员”
- 原 Pond：UI 中文名“办公区”

主 Logo 来源为 `/Volumes/SSD/Logo/PNG/纯logo.png`，已经转换并放入 `src-tauri/icons/`。界面设计基于浅色蓝紫背景、蓝色/青色/粉色信息卡、黑色强调区和圆角白色面板。

## 技术架构

- 桌面容器：Tauri 2
- 后端：Rust
- UI：React 18 + TypeScript + Vite
- 动效：Framer Motion
- 图表：Recharts
- Markdown：react-markdown
- 图标：Lucide React
- 凭据：Rust `keyring`，Windows 使用 Credential Manager
- 本地状态：Tauri app data 目录内的 JSON，临时文件写入后原子替换

采用 Tauri 而非 Electron，是为了降低安装包体积、运行内存和后台开销，同时使用 Windows 11 自带的 WebView2 保持现代 UI 与动画能力。

## 已完成能力

- 小亨对话页
- OpenAI、Anthropic、OpenAI 兼容接口、Ollama 模型调用
- 办公区、经理、职员和任务看板
- 任务创建及状态流转
- 长期记忆录入界面
- 技能开关界面
- 日程开关界面
- 设置和模型连接配置
- 本地状态持久化
- API Key 系统凭据库存储
- 响应式布局、页面切换、卡片和状态动效
- CSP 和最小 Tauri 前端权限
- NSIS EXE / MSI 构建配置

## 当前边界

这是可运行的产品 MVP，不是 OpenPiscis 全功能等价实现。以下仍需后续开发：

- 常驻自主多 Agent 调度运行时
- 经理拆解任务、职员执行任务的真实后台状态机
- MCP 客户端与工具注册中心
- 文件系统、Shell、浏览器自动化等受控工具
- 企业微信/飞书/钉钉等即时通讯入口
- 日程的后台定时执行器
- 记忆检索、压缩和注入模型上下文
- 操作审计、权限策略、任务恢复与失败重试

在这些能力实现前，不得把界面中的“技能”“日程”“职员执行”描述为已经具备真实自主执行能力。

## 核心文件

- `src/App.tsx`：页面、导航及主要交互
- `src/styles.css`：完整视觉系统和响应式规则
- `src/defaultState.ts`：角色、办公区、任务等初始数据
- `src/services/backend.ts`：前端与 Tauri 命令桥接、浏览器预览回退
- `src-tauri/src/lib.rs`：状态存储、凭据和模型请求
- `src-tauri/tauri.conf.json`：应用标识、窗口、CSP、安装包
- `scripts/bootstrap-windows.ps1`：Windows 环境部署和完整构建
- `scripts/build-windows.ps1`：已有环境下重复构建
- `.github/workflows/build-windows.yml`：GitHub Windows runner 构建
- `PROJECT_CONVERSATION.md`：已移除敏感信息的完整项目对话记录

## Windows 构建任务

在 Windows PowerShell 中进入项目目录，以管理员身份运行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\bootstrap-windows.ps1
```

完成后必须确认以下内容：

1. `npm run build` 成功。
2. `cargo fmt --check` 成功。
3. `cargo clippy -- -D warnings` 成功。
4. `npm run tauri build -- --bundles nsis,msi` 成功。
5. 启动 release exe，检查首页、聊天、办公区、任务、设置页面。
6. 使用测试模型配置验证至少一次真实对话；没有 API Key 时验证 Ollama 或只做 UI 测试。
7. 对 NSIS 安装包计算 SHA-256，并把 EXE/MSI 和哈希文件作为最终产物返回。

安装包预计位于：

```text
target\release\bundle\nsis\*.exe
target\release\bundle\msi\*.msi
```

若显式指定 `x86_64-pc-windows-msvc` target，则位于：

```text
target\x86_64-pc-windows-msvc\release\bundle\nsis\*.exe
target\x86_64-pc-windows-msvc\release\bundle\msi\*.msi
```

## 质量要求

- 不把 API Key 写入 JSON、日志或前端 localStorage。
- 不为方便开发放宽 CSP 或开放全量 Shell/文件权限。
- 不提交 `node_modules`、`target`、缓存和真实凭据。
- 修改角色文案时保持英文内部 ID 和中文 UI 名称的一致性。
- Windows 构建失败时保留完整错误输出，优先修复环境或代码，不跳过检查。
