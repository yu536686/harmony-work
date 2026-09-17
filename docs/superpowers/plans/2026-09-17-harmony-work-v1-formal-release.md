# Harmony Work v1.0.0 正式版体验重构实施计划

> **执行约束：** 先完成每个任务的失败测试，再实现最小变更使测试通过。桌面端代码位于 `E:\harmony-work`（SMB 映射为 `/Volumes/192.168.11.5/harmony-work`）；门户端代码位于 `/Users/yutianqi/Documents/亨吉利`，上线目录为 `192.168.11.29:/opt/hengjili/marketing-portal`。构建、测试和打包仅在远端 Windows 执行；不要把 `.package-target/`、`.verify-target/`、`.superpowers/` 或站点审计产物提交到 Git。

**目标：** 交付 Harmony Work `1.0.0` 第一个正式版。重组左侧导航及设置、把企业微信和个人微信配置改为卡片化自动初始化、接入官网工作空间真实账户与用量数据、把素材库升级为可筛选和预览的业务内容库，并保留已有 Skill 来源、协作模块、云端设置同步和所有高级 IM 渠道。

**设计依据：** `docs/superpowers/specs/2026-09-17-harmony-work-v1-formal-release-design.md`

**技术栈：** React 18、TypeScript、Vite、Vitest、Tauri 2、Rust、SQLite、Codex app-server、Node/Express、PostgreSQL、WPF Bootstrapper、NSIS、Windows PowerShell。

---

## 工作区职责与提交边界

| 工作区 | 职责 | 分支与提交策略 |
| --- | --- | --- |
| `/Volumes/192.168.11.5/harmony-work` | Windows 桌面端、Tauri、正式安装器 | 当前 `feat/v1-formal-release-experience`，分阶段提交；最终经线性合并进入 `main` |
| `/Users/yutianqi/Documents/亨吉利` | 门户素材分类、账户接口契约及 11.29 部署 | 建立独立 `feat/harmony-work-v1-assets-contract`，独立测试、提交和部署；不混入桌面仓库提交 |

桌面端的 `MarketingAsset` 需要能够消费旧门户响应：若 `contentClass` 或 `isAuxiliary` 缺失，采用安全白名单展示常见业务格式；一旦门户升级完成，以服务端分类为唯一判断依据。桌面 WebView 只能经 Tauri 调门户，不直接访问中转站或读取服务端凭据。

## 任务 1：建立可测试的导航状态模型并重组左侧栏

**文件：**
- 新建：`src/navigation/workspaceNavigation.ts`
- 新建：`src/navigation/workspaceNavigation.test.ts`
- 修改：`src/App.tsx`
- 修改：`src/App.css`
- 修改：`src/components/Market/index.tsx`
- 修改：`src/components/Market/Market.css`

1. 先在 `workspaceNavigation.test.ts` 写失败用例，验证工作区一级项恰为 `chat`、`creative`、`marketing-assets`、`skills`；协作组恰为 `team`、`members`、`experts`、`scheduler`、`memory`；系统组仅有 `settings`。验证旧路由 `market`、`tools`、`audit`、`debug` 分别归一到 `skills` 或 `settings` 的具体 section。
2. 在 `workspaceNavigation.ts` 定义稳定的路由与设置跳转契约，避免页面字符串散落在 `App.tsx`：

   ```ts
   export type SettingsSection =
     | "account-cloud" | "model-api" | "channels" | "workspace-security"
     | "agent-skills" | "appearance" | "system-diagnostics";

   export type WorkspaceRoute =
     | "chat" | "creative-music" | "creative-image" | "marketing-assets" | "skills"
     | "team" | "members" | "experts" | "scheduler" | "memory" | "settings" | "about";

   export function normalizeWorkspaceRoute(route: string): {
     route: WorkspaceRoute; settingsSection?: SettingsSection;
   }
   ```

3. 修改 `App.tsx`：移除 `MarketTab` 和 `marketTab` 状态；市场入口直接渲染 `Skills`。将协作组展开状态保存为 `localStorage["harmony-work.collaboration-expanded"]`，页面切换只改变当前路由，不卸载正在运行的会话容器。把工具、日志和调试的旧导航目标通过 `normalizeWorkspaceRoute` 导至设置对应 section。
4. 调整 `Market/index.tsx` 为兼容外壳或删除该层的专家/团队 Tab UI，使页面只渲染 `Skills`。`School` 和 `Pond` 仍由协作项独立渲染，成员与专家沿用 `School` 的既有子页，团队沿用 `Pond`。
5. 修改 `App.css` 和 `Market.css`：实现“工作 / 协作 / 系统”组标题、协作展开箭头、当前项状态和窄窗口溢出规则；不增加卡片嵌套或会改变会话区最小宽度的布局。
6. 运行并确认：

   ```powershell
   cd E:\harmony-work
   call E:\xiaoheng-tools\winbuild.cmd npm run test:watch -- --run src/navigation/workspaceNavigation.test.ts
   call E:\xiaoheng-tools\winbuild.cmd npm run build:web
   ```

7. 提交：`feat(导航): 重组工作区与协作入口`。

## 任务 2：把市场收敛为 Skill 生命周期工作台

**文件：**
- 新建：`src/components/Skills/skillLifecycle.ts`
- 新建：`src/components/Skills/skillLifecycle.test.ts`
- 修改：`src/components/Skills/index.tsx`
- 修改：`src/components/Skills/Skills.css`
- 修改：`src/services/tauri/config.ts`
- 修改：`src-tauri/src/commands/config/skills.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`

1. 先编写 `skillLifecycle.test.ts`，覆盖四态计算：`installed`、`enabled`、`dependenciesReady`、`runtimeVerified`。用例必须断言“已安装但禁用”、“启用但 Node/MCP 缺失”、“依赖完整但运行时尚未验证”均不可标记为可调用。
2. 新增前端纯函数和类型：

   ```ts
   export type SkillRuntimeState = "unverified" | "ready" | "repair-required";
   export type SkillLifecycle = {
     installed: boolean; enabled: boolean; dependenciesReady: boolean;
     runtimeState: SkillRuntimeState; repairHint?: string;
   };
   export function canInvokeSkill(state: SkillLifecycle): boolean;
   ```

3. 在 Rust `skills.rs` 增加只读 `verify_skill_runtime` 命令。它读取已安装 Skill 元数据、检查声明的依赖和本地/MCP 发现状态，但不得执行 Skill 的用户业务命令。为成功、缺失 `SKILL.md`、缺少命令依赖和未启用的记录添加 `#[cfg(test)]` 覆盖。
4. 在 `config.ts` 添加 `skillsApi.verifyRuntime(id)`，并让安装链路按“兼容性检查 -> 安装 -> 同步磁盘 -> 启用 -> 运行时验证”刷新同一条记录。`bootstrap.rs` 注册该命令。
5. 重构 `Skills/index.tsx` 为三个固定视图：`已安装`、`发现技能`、`诊断与调用`。发现技能继续保留本地路径/直链、ClawHub、Harmony 市场、Anthropic 官方插件、OpenAI curated skills 五个来源；它们不能再与已安装列表混排。诊断项提供明确修复动作，不把失败伪装成“已安装”。
6. 运行 `skillLifecycle.test.ts`、Rust 单测和全量前端测试；手工安装一个低风险本地 Skill，确认 Codex runtime 列表只暴露验证通过且启用的项。
7. 提交：`feat(技能): 增加可调用状态与诊断工作台`。

## 任务 3：将设置页拆为二级信息架构并收纳系统页面

**文件：**
- 新建：`src/components/Settings/settingsSections.ts`
- 新建：`src/components/Settings/settingsSections.test.ts`
- 新建：`src/components/Settings/SettingsNavigation.tsx`
- 新建：`src/components/Settings/SettingsNavigation.css`
- 修改：`src/components/Settings/index.tsx`
- 修改：`src/components/Settings/Settings.css`
- 修改：`src/components/Tools/index.tsx`
- 修改：`src/components/AuditLog/index.tsx`
- 修改：`src/components/DebugPanel/index.tsx`

1. 先写 `settingsSections.test.ts`，断言七个 section 的顺序与映射稳定：账户与云端、模型与 API、渠道与连接、工作区与安全、Agent 与技能、外观与交互、系统与诊断。断言 `tools`、`audit`、`debug` 都只属于系统与诊断，不能重新出现在一级导航。
2. 在 `settingsSections.ts` 把 section 的 id、中文标签、图标和原设置字段归属声明为数据。`SettingsNavigation` 通过该数据渲染侧栏，并接受 `initialSection?: SettingsSection`；键盘和窄布局中仍可访问全部 section。
3. 在 `Settings/index.tsx` 给现有设置块添加稳定 section 锚点和 `aria-labelledby`，先做收纳而不重写已有表单。保留默认 `context_window: 128000`、`confirm_shell_commands: false`、文件写入确认和外部工作区风险说明。
4. 将 `Tools`、`AuditLog`、`DebugPanel` 改为可嵌入模式（例如 `embedded` 与 `hidePageHeader`），由“系统与诊断” section 复用。旧直达路由必须仍能跳到同一 section。
5. 手工验证窗口宽度 `980px`、常规桌面宽度和设置深链：二级菜单可滚动，设置内容不会被侧栏遮挡，工具/日志/调试不丢失功能。
6. 运行设置单测与 `npm run build:web`。
7. 提交：`feat(设置): 建立分类导航并收纳系统诊断`。

## 任务 4：统一企业微信与个人微信的配置、初始化和连接状态

**文件：**
- 新建：`src/components/Settings/channelLifecycle.ts`
- 新建：`src/components/Settings/channelLifecycle.test.ts`
- 修改：`src/components/Settings/index.tsx`
- 修改：`src/services/tauri/config.ts`
- 修改：`src-tauri/src/commands/config/enterprise_capability.rs`
- 修改：`src-tauri/src/commands/config/settings.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`

1. 先写 `channelLifecycle.test.ts`，覆盖两个主渠道共享状态：`unconfigured`、`initializing`、`awaiting-user-action`、`ready`、`repair-required`、`connected`。验证失败会保留安全的诊断摘要，且成功路径不要求用户到会话列表再次点击“连接渠道”。
2. 定义跨 UI/命令层 DTO：

   ```ts
   export type PrimaryChannelId = "wecom" | "wechat";
   export type ChannelSetupState =
     | "unconfigured" | "initializing" | "awaiting-user-action"
     | "ready" | "repair-required" | "connected";
   export type ChannelSetupResult = {
     channel: PrimaryChannelId; state: ChannelSetupState;
     capabilities: string[]; nextAction?: string; diagnostic?: string;
   };
   ```

3. 在 `enterprise_capability.rs` 增加企业微信的组合初始化命令：读取已加密保存的 Bot ID/Secret，检测内置 Node、`@wecom/cli`、凭据和命令能力；只执行官方允许的非交互初始化；刷新能力列表后调用现有 gateway 连接。官方 CLI 需要人工授权时返回 `awaiting-user-action` 与唯一操作说明，不伪造已连接。
4. 复用现有 `wechatApi.startLogin`/轮询逻辑，将扫码成功后的 token、bot id、base URL 加密保存、监听启动与 `gatewayApi.connect()` 收敛为一个成功分支。处理二维码过期、端口占用、令牌失效和网关失败，分别映射可重试或可修复的状态。
5. 调整“渠道与连接” section：企业微信和个人微信作为两张主卡，保存后立即驱动初始化并显示运行状态；飞书、钉钉、Telegram、Slack、Discord、Teams、Matrix、Webhook、邮件保留在“高级集成”折叠区。不得删除既有字段或迁移已有渠道设置。
6. 使用 Rust 单测模拟 CLI 检测与网关结果；在 Windows 真实环境验证无凭据、CLI 需授权和有效凭据三种企业微信路径，并验证个人微信扫码完成后会话无需二次连接。
7. 提交：`feat(IM): 自动化主渠道配置与连接恢复`。

## 任务 5：新增个人账户与用量页面，复用官网工作空间数据

**文件：**
- 新建：`src/components/Account/index.tsx`
- 新建：`src/components/Account/Account.css`
- 新建：`src/components/Account/Account.test.tsx`
- 修改：`src/App.tsx`
- 修改：`src/services/tauri/marketing.ts`
- 修改：`src-tauri/src/commands/marketing.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`

1. 先编写 `Account.test.tsx`：成功加载时显示姓名/审批状态、积分、今日消耗、调用次数、Token、掩码 key 和云端配置状态；任一统计请求失败时其他已成功块继续展示；401/403 显示登录或审批恢复入口；默认渲染绝不显示完整 API Key。
2. 在 `marketing.ts` 定义 `AccountOverview` 和 `marketingApi.accountOverview()`。返回数据使用门户字段名映射，但不把 API Key 写入 localStorage、会话历史或遥测：

   ```ts
   export type AccountOverview = {
     user: MarketingUser;
     balance: number | null;
     usage: Record<string, number> | null;
     gatewayApiKey: string | null;
     cloudSettings: AccountSettingsSyncStatus;
   };
   ```

3. 在 Rust `marketing.rs` 创建一个 Tauri 命令：沿用营销门户已认证会话，并发请求 `/api/auth/me`、`/api/gateway/balance`、`/api/gateway/usage`、`/api/gateway/credentials`，将非关键统计错误降级为 `null`，身份失败保持鉴权语义。为 JSON 规整、部分失败、401 和 key 不落盘写单测；在 `bootstrap.rs` 注册命令。
4. 实现 `Account` 页面。头像菜单“个人账户”改为打开该页，不再跳转营销工作台；API Key 默认遮罩，仅在用户明确点击“显示”后短暂可见，复制操作只使用剪贴板。提供“将 Harmony 凭据用于本机”的确认对话，确认后才更新当前用户的本地 provider 设置。
5. 账户页展示现有 `accountSettingsSyncApi.status()` 的云端快照、默认模型和同步入口，不复制或重造云存档实现。
6. 在真实门户登录会话下验证端到端数据与 key 掩码，在已退出会话下验证恢复提示。
7. 提交：`feat(账户): 接入门户工作空间用量与凭据`。

## 任务 6：在门户端建立素材“正式内容/辅助文件”契约

**门户分支文件：**
- 新建：`src/server/assets-classification.ts`
- 新建：`src/server/assets-classification.test.ts`
- 修改：`src/server/assets.ts`
- 修改：`src/server/db.ts`
- 修改：`src/server/index.ts`

1. 先写 `assets-classification.test.ts`。用表驱动覆盖：`.nfo`、隐藏文件、`.DS_Store`、`Thumbs.db`、Jellyfin 元数据目录、封面/海报/缩略图、字幕与歌词副本必须是 `isAuxiliary: true`；JPG/MP4/MP3/PDF/Office 和明确登记的工程文件必须归为 `business` 或 `source`。
2. 定义唯一分类器，避免扫描、接口和桌面各自猜测：

   ```ts
   export type AssetContentClass = "business" | "source" | "auxiliary";
   export type AssetClassification = { contentClass: AssetContentClass; isAuxiliary: boolean };
   export function classifyAssetContent(input: { fileName: string; relativePath: string; extension: string }): AssetClassification;
   ```

3. 在 `db.ts` 的幂等 schema 中为 `asset_files` 增加 `content_class TEXT NOT NULL DEFAULT 'business'` 与 `is_auxiliary BOOLEAN NOT NULL DEFAULT FALSE`，以及适合 `is_available/is_auxiliary/content_class` 查询的索引；现有记录以可重复运行的更新语句回填分类。
4. 修改 `assets.ts` 的 `walkFiles` 和 `indexAssetFile`：扫描时跳过系统文件，并对可索引文件写入分类字段。明确允许业务封面由管理员标记为正式内容，不通过命名绕过规则。保留 NAS 根目录缺失时不批量标记失效的现有保护。
5. 修改门户 `index.ts` 的 `/api/assets`、`/api/assets/formats`、`/api/assets/summary` 和 `assetRow`：默认追加 `is_auxiliary = FALSE`，支持 `includeAuxiliary` 与 `contentClass` 显式查询，响应包含 `contentClass/isAuxiliary`。格式统计和摘要与列表使用同一默认过滤条件。
6. 运行门户的分类、路由和 TypeScript 测试；在 11.29 预发布环境执行 migration 和单次扫描，验证 `.nfo` 不在默认 `/api/assets` 和 `/api/assets/formats` 中，业务视频/图片仍能读取。
7. 将门户变更提交、推送、部署到 `/opt/hengjili/marketing-portal`，重启 `hengjili-marketing-portal.service`，检查健康接口、素材 API 鉴权和真实 NAS 素材数。门户变更独立提交：`feat(素材): 区分业务内容与媒体辅助文件`。

## 任务 7：升级桌面素材库的筛选与预览体验

**文件：**
- 新建：`src/components/Marketing/assetFiltering.ts`
- 新建：`src/components/Marketing/assetFiltering.test.ts`
- 修改：`src/services/tauri/marketing.ts`
- 修改：`src-tauri/src/commands/marketing.rs`
- 修改：`src-tauri/src/app/bootstrap.rs`
- 修改：`src/components/Marketing.tsx`
- 修改：`src/components/Marketing.css`

1. 先写 `assetFiltering.test.ts`：门户含 `isAuxiliary=true` 的项永不出现在默认列表；旧门户未提供分类字段时，仅图片/视频/音频/PDF/Office/明确工程格式可见；筛选切换或分页返回后选中项不存在时必须清空预览选择。
2. 扩展 `MarketingAsset` 和 `MarketingAssetQuery`：

   ```ts
   contentClass?: "business" | "source" | "auxiliary";
   isAuxiliary?: boolean;
   includeAuxiliary?: boolean;
   ```

   `normalizeAsset` 保留门户字段，Rust `marketing.rs` 透传 `contentClass/includeAuxiliary` 查询参数，并在 `bootstrap.rs` 保持命令注册。
3. 在 `Marketing.tsx` 让默认查询传递 `includeAuxiliary: false`。格式过滤改为图片、视频、音频、文档、工程源文件、归档；工程源文件默认不勾选。常用格式、文件夹、搜索、排序和分页使用同一查询状态，取消过期网络响应。
4. 预览规则：图片用现有安全 data URL；视频/音频分别使用有控件播放器；PDF 用安全预览通道；Office/工程文件展示缩略信息、下载与“放入会话”；任意预览错误仍显示文件名、类型、路径和放入会话按钮。不得把门户 URL 交给不受控 `iframe`。
5. 在窄宽度和全屏宽度检查网格、筛选器、详情面板、空状态与错误状态，确认 `980px` 最小窗口内不截断主要操作。
6. 运行素材筛选单测、`npm test`，并使用门户部署后的真实 PDF、图片、视频、`.nfo` 和 PSD 验收。
7. 提交：`feat(素材库): 优化业务筛选与多格式预览`。

## 任务 8：版本、安装器和升级兼容性提升至正式版

**文件：**
- 修改：`package.json`
- 修改：`package-lock.json`
- 修改：`src-tauri/Cargo.toml`
- 修改：`src-tauri/tauri.conf.json`
- 修改：`installer/WorkBuddyBootstrapper/WorkBuddyBootstrapper.csproj`
- 修改：`installer/WorkBuddyBootstrapper/InstallerEngine.cs`
- 修改：`scripts/offline-runtime-lock.test.mjs`
- 修改：`scripts/build-full-installer.ps1`
- 修改：`README.md` 与用户可见更新说明（若当前版本号有明确展示）

1. 先更新 `offline-runtime-lock.test.mjs` 为失败断言：`package.json`、Tauri、Cargo、payload manifest 和最终安装器文件名均为 `1.0.0`；Tauri identifier 仍为 `com.harmony.xiaoheng-workbuddy`，产品名仍为 Harmony Work。
2. 将所有版本源统一为 `1.0.0`，使用 `npm install --package-lock-only` 更新 lockfile；不更改包标识、设置目录、runtime 根目录或已安装 Skill 路径。
3. 检查 WPF Bootstrapper 的 `InstallerVersion` 和缓存根路径。为从 `1.3.0-alpha.2` 升至 `1.0.0` 增加升级兼容测试：正式版必须重新解压新的 payload manifest，但不得删除已存在的用户 runtime、应用设置、IM 凭据和已安装 Skills。缓存键可隔离版本，不使用“版本号更高才安装”的比较来跳过正式版。
4. 更新 `build-full-installer.ps1` 发布文件命名并验证生成 `Harmony-Work-1.0.0-x64-setup.exe`；保持内置 Node、Python、Git、wecom CLI 和离线 wheel 的锁定/校验机制不变。
5. 运行安装器契约测试、WPF 项目编译，并在 Windows 虚拟/测试用户目录执行“已装 alpha -> 安装 1.0.0 -> 首次启动”的回归：应用启动、配置仍在、工具运行时可发现。
6. 提交：`release: 准备 Harmony Work 1.0.0 正式版`。

## 任务 9：整体验收、正式安装器构建和发布证据

**文件：**
- 修改：`docs/release-notes/1.0.0.md`（若目录不存在则新建）
- 修改：`docs/superpowers/specs/2026-09-17-harmony-work-v1-formal-release-design.md`（仅将状态更新为已验收，并补充实际证据链接）

1. 在远端 Windows 依次运行：

   ```powershell
   cd E:\harmony-work
   call E:\xiaoheng-tools\winbuild.cmd npm test
   call E:\xiaoheng-tools\winbuild.cmd npm run build
   call E:\xiaoheng-tools\winbuild.cmd npm run tauri build -- --bundles nsis
   call E:\xiaoheng-tools\winbuild.cmd powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-full-installer.ps1 -SkipDownload -InnerInstaller "E:\harmony-work\.package-target\release\bundle\nsis\Harmony Work_1.0.0_x64-setup.exe" -OutputDirectory "E:\harmony-work\release\full" -Version "1.0.0"
   Get-FileHash E:\harmony-work\release\full\Harmony-Work-1.0.0-x64-setup.exe -Algorithm SHA256
   ```

2. 完成安装器的真实用户路径验收：双击立即显示 Bootstrapper；环境探测只安装缺失项目；进度不泄漏 Python 等内部实现；安装完成后立即启动可关闭安装器；应用可拖放附件、能打开成果、可预览素材。
3. 完成产品验收：导航与设置 section、Skill 五来源与运行时验证、WeCom 自动初始化、微信扫码自动连接、个人账户用量、默认素材过滤、各类预览、云端设置同步均按设计规格检查。
4. 写入简短发布说明、安装器绝对路径、文件大小、SHA-256、测试命令与已知非阻塞限制。提交发布说明和验收状态，不提交生成安装包、缓存或临时截屏。
5. 在请求合并时执行 `git diff --check`、检查工作树仅有允许的变更、线性合并 feature 分支到 `main`、推送 GitHub，并创建 `v1.0.0` Release 后上传已校验的安装器。

## 最终验收清单

- [ ] 市场只呈现 Skill 生命周期；五个技能来源、安装、启用、诊断和运行时验证均可用。
- [ ] 协作组含团队、成员、专家、定时任务、记忆；工具、日志、调试仅在设置的系统与诊断中出现。
- [ ] WeCom 保存凭据后完成可自动步骤并自动连接；微信扫码完成后不需会话页二次连接；高级 IM 仍可配置。
- [ ] 账户页显示门户真实身份、积分、用量、Token、掩码 API Key、云端配置状态，且不暴露服务端凭据。
- [ ] 默认素材列表不含 `.nfo`、Jellyfin 元数据、系统文件、缩略图或封面辅助项；筛选、分页和预览可用。
- [ ] `npm test`、生产构建、NSIS、完整 WPF 安装器构建、alpha 升级到 `1.0.0` 回归全部通过。
- [ ] 正式交付物为 `E:\harmony-work\release\full\Harmony-Work-1.0.0-x64-setup.exe`，附 SHA-256。
