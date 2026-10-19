# Windows 轻量更新退出后停留在旧版：修复与验收（2026-10-19）

## 根因

192.168.11.5 的 Admin 安装日志在 17:48:58、17:51:09 记录主程序文件占用，覆盖和恢复均失败。32 位 NSIS 通过 `$SYSDIR` 调用到 32 位 PowerShell；它读取 64 位 Harmony Work 的 `Get-Process.Path` 返回空值，因此原先按安装路径筛选进程时漏检。完整 Python 布局校验约需 50–75 秒，期间重新打开旧客户端会再次占用主程序。

## 修复

在 x64 Windows 上通过 Sysnative 启动 64 位 PowerShell，脚本直接被 32 位宿主执行时立即拒绝。自动更新在校验前、覆盖前等待同路径客户端退出；对将替换的 EXE 做独占访问探测后才恢复旧事务和写入。保留原有签名、依赖哈希、备份、回滚及启动确认逻辑。自动安装显示无额外确认操作的 NSIS 进度页，成功关闭，失败保留详情。

## 实机证据

- 隔离 SMB 账号使用与 Admin 一致的完整 Python 3.12.8 布局：6057 个运行时文件校验通过。
- 实际调用 1.1.1 客户端 update_install，从腾讯云 CDN 下载 RC 轻量包，完成校验、主程序退出、覆盖、自动重启和启动稳定性确认。18:08:14 安装日志 SUCCESS version=1.1.2-rc.1。
- 预置中断事务在同次更新中于 18:07:57 RECOVERED，未删除备份来规避问题。
- 额外自动安装回归：先保持客户端运行，确认等待；退出后进入哈希校验，再重新打开客户端。18:10:39 再次记录 Waiting for Harmony Work to exit；正常退出测试客户端后，18:11:19 SUCCESS version=1.1.2-rc.1。
- 会话 70736318-2ed6-4bab-a873-065263b4a729 和配置 max_iterations=37 保留；工作区 keep.txt SHA-256 仍为 D2416CDBDBB1A9B037B01317AACDC2AA24E58B71D565743A08A441287C3B7D7D。
- 交互桌面 session=1 以无效安装目录安全探测进度窗口，获得实际 HWND=8327528，测试任务退出 0 并清理；没有升级或退出 Admin 正在运行的客户端。SSH 非交互会话 MainWindowHandle=0 不能用于判断用户桌面是否显示窗口。
- 12 个 manifest/所有权/兼容性测试通过；Windows 进程、锁和 32 位宿主回归四项通过。
- 两个最终轻量包签名正常，篡改一个字节被验证器拒绝。腾讯云 CDN 全量下载后的大小和 SHA-256 与本地一致。

最终显式声明 Page instfiles 后，NSIS 产生的 r4 字节与已完成上述实际安装的 r3 完全一致（NSIS 原先已隐式生成默认安装页）。保留显式声明，并使用新的不可变 r4 文件名发布。没有通过修改同一 CDN 对象内容替换旧包。

## 最终发布

- 正式版轻量包：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.1-x64-light-r4.exe
  - 17515408 bytes
  - SHA-256 6ca1843138b03fd90f0a581d263a5694ade873ff764da35f80d428f46ac95e77
- 测试版轻量包：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-rc.1-x64-light-r4.exe
  - 17515279 bytes
  - SHA-256 488394e2f565c67b95ec73a5cedc3fd93cfb929b964c3c17f24f75b48167f3a1
- stable/preview 更新清单指向各自 r4 CDN 链接，公开 GET 与本地签名清单一致。
- 官网 https://harmonywork.com.cn/workbuddy#download 的轻量包元数据指向正式版 r4，公开 API 已核对。完整安装包保持原 794119729 bytes / SHA-256 2cd88b8297a1fa81a6a60321f7a7a28c3a37d0806c9895b86869c89f6e721f43。

轻量包仅覆盖 18 个应用自有文件，继续复用 Node、Python、Git、WebView2 等运行环境，不清除用户会话、配置或工作区。Admin 的实际升级尚待用户重试；完整安装与重新打开进程的回归发生在隔离账号，不能等同于已替用户升级。
