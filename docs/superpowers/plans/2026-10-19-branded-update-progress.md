# 简约品牌更新窗口实现计划

> 当前会话使用 executing-plans 内联完成，复用既有隔离分支。

**目标：** 用品牌进度卡片替代 NSIS 传统页，同时维持轻量安装安全性。
**架构：** Windows 自带 WPF launcher + 独立事务 worker + 原子进度文件；现有 NSIS 静默封装，客户端仅传外观。
**技术栈：** Windows PowerShell 5.1、WPF、NSIS、现有 Rust/Tauri 与 React。

### 1. 协议与 worker
- [ ] 新建 scripts/test-light-progress.ps1：真实写入/读取计数和失败恢复字段、拒绝无效计数、原子覆盖测试，运行先失败。
- [ ] installer/light/progress.ps1 输出 schema=1、phase、detail、completed、total、terminal、safeRestart、restartPath。worker 添加 ProgressFile 并从校验/等待/复制/健康/回滚发布。无 ProgressFile 维持原行为。
- [ ] 运行 Windows 协议测试及原有 test-light-handoff.ps1，确认进程和锁保护仍有效。

### 2. 品牌窗口
- [ ] 新建 launcher.ps1、ProgressWindow.xaml，沿用客户端主题 token 和 PNG。以 DispatcherTimer 轮询子进程状态，子进程只运行固定 worker。所有线程更新走 WPF Dispatcher。
- [ ] 不可计量阶段 IsIndeterminate=true；文件阶段显示 completed/total 与阶段百分比；终止状态与退出码均成功才自动关闭。
- [ ] 失败提供 Retry / Log / Close；关闭时安全恢复旧客户端；错误详情不伪造成功。保持 CHECK 完全无 GUI。
- [ ] overlay.nsi 静默运行 launcher，通过 Sysnative x64 STA；拷贝新增脚本、XAML、品牌图片。

### 3. 客户端外观
- [ ] 先测试 appearance 只允许 violet/gold + light/dark。新 update_install 可选 appearance 参数，持久化到 updates/appearance.json；原子写入无配置或凭据泄漏。
- [ ] nativeUpdateStore 在安装时读取 DOM 当前主题并发送，补充前端测试。已有客户端缺少外观字段走系统 fallback。
- [ ] 运行定向 Rust 和前端测试，在 Windows 缓存构建 stable/RC 两个新主程序，替换各自冻结 payload 的主程序，不修改第三方组件。

### 4. 实机与发布
- [ ] Windows 构建两份 r5 包，签名并验证原包和篡改字节。交互桌面检查窗口浅/深色、失败/关闭/重试、CHECK 无 GUI。
- [ ] 隔离账号安装 stable 后走真实 CDN 更新到 RC，检查版本、健康回执、会话/配置/工作区与运行环境。
- [ ] 先上传不可变 r5 EXE 并全量下载核对 SHA，最后切换 stable/preview JSON 和官网轻量入口。保持既有 full 包。
- [ ] 清理测试客户端/任务/临时凭据，提交推送代码和验收报告。
