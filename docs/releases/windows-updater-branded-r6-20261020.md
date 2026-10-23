# 简约品牌 Windows 更新窗口：实机验收与发布（2026-10-20）

用户已选择简约品牌进度窗口。正式版仍为 1.1.1，测试版递增到 1.1.2-rc.2，使已安装 rc.1 的客户端可按正常版本规则收到下一次更新。源分支仍沿用 feat/windows-light-updater-20261019 和 test/windows-updater-1.1.2-rc.1。

## 实现

- 传统 NSIS 页完全隐藏，仅负责解包；x64 STA PowerShell WPF 显示 500×346 的圆角品牌卡片。使用系统自带组件，没有新增 .NET 或浏览器运行时。
- 浅/深色、紫色/黑金配色，品牌图标覆盖窗口及任务栏。新客户端通过安装请求传递当前主题；旧客户端首次升级缺少该字段时采用系统配色。
- 校验、复制显示真实阶段计数与百分比；等待退出、恢复、启动确认等不可计量阶段显示动态进度。安装进行中可最小化，阻止关闭；失败保留提示并提供重试、日志、关闭。成功收到客户端健康回执并稳定运行后自动关闭。
- GUI 与事务 worker 独立。保持完整性验证、64 位宿主、进程/文件锁等待、原子 journal、备份、回滚及健康 token。失败时仅在回滚安全且注册安装目录与旧 EXE 身份再验证通过后返回旧客户端。
- 修复了额外审查发现的 UI 问题：worker 意外退出时，旧非终止进度不能覆盖失败提示。真实窗口回归先复现 exit 8，再验证修复。

## 验证证据

- 8 项定向 Rust updater 测试、11 项前端 updater 测试、12 项轻量包 manifest/所有权测试通过。
- Windows 6 项进度协议测试、4 项运行进程/文件锁/32 位宿主回归通过。
- 真实 WPF 已报告失败与意外退出两种生命周期测试通过：真实计数、安装中阻止关闭、失败后重试、新 worker 成功后自动关闭。
- Windows WPF 四种配色均实际加载并渲染；截图存于本任务 output/windows-branded-ui。
- 隔离 SMB 账号使用完整 Python 布局，6057 个运行时文件校验通过；18 个应用自有文件覆盖。嵌入式 Python 兼容配置仍有 1678 个精确哈希条目，未放宽校验。
- 02:29:17.891：1.1.1 生产包安装 SUCCESS，NSIS 退出 0。实际页面 URL 为 http://tauri.localhost/，root 已渲染。健康回执来自客户端自身，测试没有调用 update_status 人为制造回执。
- 经真实“关于 → 检查更新 → 立即更新”按钮发起到 rc.2 的 CDN 更新。安装请求实际写入 {theme:violet,colorMode:dark}；新客户端保留深色模式。
- 02:39:34：静默 CHECK 校验完整 Python 配置通过；02:39:37 独立更新 worker 接管；02:40:43.689：SUCCESS version=1.1.2-rc.2。新版生产 URL 和 root 渲染通过，安装器随后退出。
- 会话 70736318-2ed6-4bab-a873-065263b4a729 / UPDATER-PRESERVATION-20261019、max_iterations=37 均保留。工作区 keep.txt SHA-256 仍为 D2416CDBDBB1A9B037B01317AACDC2AA24E58B71D565743A08A441287C3B7D7D。
- Node、Python、Git、浏览器和技能列表与基线一致；未做全产品每项功能的穷举测试。
- 两个包的原始签名均通过，篡改一个字节均被拒绝；腾讯云 CDN 全量下载后的大小和 SHA-256 与本地完全一致。

## 被拦截的候选构建

未启用 tauri/custom-protocol 的 R5 候选尝试连接开发服务，真实健康验收拒绝该包，02:17:21 回滚成功，原版本与数据恢复。R5 从未进入公开更新 feed；上传的未引用 COS 对象已删除。最终 R6 明确使用 cargo --release --features tauri/custom-protocol，并以生产 URL、已渲染页面及客户端自行发出健康回执为验证条件。

正式版最终包只在 GUI 配色/任务栏图标完成视觉验收后重封装；已成功实机安装的 native 主程序及事务 worker 保持相同。

## 最终发布

- 1.1.1：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.1-x64-light-r6.exe
  - 18804273 bytes
  - SHA-256 `2c7ee6855d850f423a4b17e6fad8480c3f35a0262e79cf7575209d0f7847554f`
- 1.1.2-rc.2：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-rc.2-x64-light-r6.exe
  - 18797261 bytes
  - SHA-256 `bc41c794bf03e1e0a42cb51899859e86facd9a28f6435c5fb502f3d3d83a875e`

稳定/测试频道分别指向对应 R6 CDN 对象，公开 JSON 已与本地签名清单核对。官网 https://harmonywork.com.cn/workbuddy#download 的“轻量安装包”入口指向正式版 R6，公开 /api/workbuddy/release 已确认。完整安装包仍为原 794119729 bytes / SHA-256 2cd88b8297a1fa81a6a60321f7a7a28c3a37d0806c9895b86869c89f6e721f43。

测试仅覆盖隔离账号。当前 Admin 用户的客户端保持运行，未替用户退出或升级。测试客户端已正常退出，SMB 原始嵌入式 Python 已恢复，本次完整 Python 测试副本及中止候选临时目录已清理。旧的完整 Python 参考目录仍保留供后续回归使用。
