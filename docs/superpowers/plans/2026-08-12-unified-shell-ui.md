# 统一工作台 UI 实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将应用重构为单一全局侧栏，并完整统一会话页与业务页面的视觉和交互层级。

**架构：** App 提供全局侧栏和会话挂载节点，Chat 保留全部业务状态并通过 React Portal 输出会话导航。CSS 使用 `design.md` 与 `theme.css` 的语义 Token 重写，不改动后端接口。

**技术栈：** React 18、TypeScript、Redux Toolkit、Vitest、Tauri 2、Lucide React、CSS。

---

### 任务 1：全局会话导航

**文件：**
- 创建：`src/components/Chat/GlobalSessionRail.tsx`
- 创建：`src/components/Chat/GlobalSessionRail.test.tsx`
- 修改：`src/components/Chat/SessionPicker.tsx`

- [ ] 编写会话导航渲染与收起状态测试。
- [ ] 运行测试并确认因组件缺失而失败。
- [ ] 实现筛选、新建、列表、删除和收起态。
- [ ] 运行测试并确认通过。

### 任务 2：应用外壳融合

**文件：**
- 修改：`src/App.tsx`
- 修改：`src/components/Chat/index.tsx`
- 修改：`src/App.css`
- 修改：`src/components/Chat/Chat.css`

- [ ] 在 App 中建立会话导航宿主节点。
- [ ] 通过 Portal 将 Chat 会话导航迁入全局侧栏。
- [ ] 删除 Chat 的二级侧栏并接通会话跳转。
- [ ] 使用 Lucide 替换结构 Emoji。

### 任务 3：统一页面与会话视觉

**文件：**
- 修改：`src/App.css`
- 修改：`src/components/Chat/Chat.css`
- 修改：`src/theme.css`
- 修改：`design.md`

- [ ] 重做侧栏模块区、会话区和底部账户区比例。
- [ ] 重做消息流、AI 回复、用户气泡、任务过程和输入区。
- [ ] 统一页头、页面内容区、卡片、按钮、表单、弹窗和锁定页。
- [ ] 添加小屏适配、深色模式和 reduced-motion。

### 任务 4：版本、验证与交付

**文件：**
- 修改：`package.json`
- 修改：`package-lock.json`
- 修改：`src-tauri/Cargo.toml`
- 修改：`src-tauri/tauri.conf.json`

- [ ] 将版本推进至 `0.8.7-alpha.0`。
- [ ] 运行 `npm test`，预期全部通过。
- [ ] 运行 `npm run build:web`，预期退出码 0。
- [ ] 启动页面并截图核对全局侧栏与会话布局。
- [ ] 运行 NSIS 构建并记录安装包路径与 SHA-256。
- [ ] 提交 Git，提交信息为 `feat(ui): unify app shell and conversation navigation`。
