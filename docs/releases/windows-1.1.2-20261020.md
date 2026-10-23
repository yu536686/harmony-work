# Windows 1.1.2 正式发布（2026-10-20）

从 1.1.2-rc.2 验收完成的更新器功能提升到 1.1.2；正式源码同时保留既有发布说明及进度窗口回归测试。包含关于页面检查更新、后台检测与品牌提示、stable/preview 频道、腾讯云 CDN 签名下载、轻量覆盖安装、品牌进度窗口、启动健康确认与失败回滚。

## 构建约束

- package/package-lock/Cargo/Tauri/全量安装器/卸载器版本统一为 1.1.2；轻量构建规则明确接受正式版。
- Windows 主程序生产构建使用 `cargo build --release --target x86_64-pc-windows-msvc --features tauri/custom-protocol`。
- 内层 NSIS 由 1.1.2 生产 EXE 与冻结资源重新打包；外层全量安装器重新嵌入内层 NSIS、卸载器和完整离线运行环境，不复用旧版本内层程序。
- 轻量包重新生成自有文件哈希，精确核对冻结资源，并兼容标准/嵌入式 Python 两种经过验证的运行环境布局。
- 离线运行时从既有完整载荷复用，先核对 runtime-lock、Harmony Cut requirements 与每个归档 SHA-256；未复用旧主程序、内层安装器或卸载器。
- 组装暂存资源严格采用 Tauri bundle.resources；未安装的内嵌源码/构建中间文件不能混入外置兼容依赖。
- 包策略与依赖迁移限制见 [Windows 更新包发布规则](windows-update-package-policy.md)。

## 验证与上线

- 前端 204 项测试、Node 合约 35 项和轻量清单 12 项测试通过；Windows 原生 265 项测试通过。Mac 原生套件有两项现有测试写死 Windows 绝对路径而失败，未修改无关业务代码；本次发布以 Windows 实测为准。
- Windows 6 项进度协议测试通过；真实 WPF 在已报告错误与 worker 意外退出两种场景中，均通过进度、阻止中途关闭、失败重试和成功关闭测试。
- 全量包：将隔离 SMB 账号的原程序和 runtime 移到备份位置，使用发布程序集对应的 InstallerEngine 和同一份嵌入 payload.zip 完成全部离线组件及程序安装。全新安装主程序为 1.1.2，实际 URL 为 http://tauri.localhost/，页面已渲染；Node 22.14.0、Python 3.12.8、Git 2.47.1、浏览器可用。发布 WPF 程序集经进程内界面测试达到“立即安装/准备就绪”；SSH 跨进程窗口句柄自动化不可用，改为直接检查发布程序集。
- 新装完整环境的轻量 CHECK 返回 0；原 rc.2 嵌入式 Python 环境的 CHECK 同样返回 0。清单包含 18 个自有文件、189 个复用资源，以及嵌入式 Python 1678 条和标准 Python 6057 条运行时哈希。
- 恢复 rc.2 基线后，通过实际“关于 → 检查更新 → 立即更新”按钮发现 1.1.2，下载地址为腾讯云 CDN；15:46:50 启动新版，15:47:06.229 收到正确健康回执并 SUCCESS version=1.1.2。观察到生产 URL、渲染页面和持续运行，未调用 update_status 人为制造健康回执。
- 会话 70736318-2ed6-4bab-a873-065263b4a729 / UPDATER-PRESERVATION-20261019 保留，max_iterations=37 保留，紫罗兰深色主题保留；工作区 keep.txt SHA-256 为 D2416CDBDBB1A9B037B01317AACDC2AA24E58B71D565743A08A441287C3B7D7D。
- 轻量包签名接受原包、拒绝单字节篡改；两个包从腾讯云 CDN 完整下载，大小和 SHA-256 均与本地一致。官网服务器也完整下载并核对全量包。
- stable、preview 两个 feed 最终均为 1.1.2，更新 URL 为腾讯云 CDN。官网公开 API 为 v1.1.2，含全量与轻量元数据；实际完整下载端点返回 794308145 bytes 和正确 1.1.2 文件名。
- 官网发布记录：a1f4c49d-8204-47d1-81fc-9dbbda2acf21。官网轻量入口代码已合入 GitHub main：855973d。
- 仅操作隔离 SMB 账号；Admin 当前客户端未退出或被升级。验证范围是安装、启动、核心运行时、更新与数据保留，并非每项产品能力的穷举测试。

## 安装包

| 包 | bytes | SHA-256 |
| --- | ---: | --- |
| Harmony-Work-1.1.2-x64-setup.exe | 794308145 | 786fb58e46490b819fc435d569847df53127841d70835d5100228a4dafbdbff6 |
| Harmony-Work-1.1.2-x64-light.exe | 18798037 | 7b02ec328680721a78e6cd74c4aca280630a749c4879653e06d3d8641a8d7d1b |

- 全量包：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-x64-setup.exe
- 轻量包：https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-x64-light.exe
- 官网：https://harmonywork.com.cn/workbuddy#download
- 全量包服务器源文件：/opt/hengjili/updater-staging/Harmony-Work-1.1.2-x64-setup.exe；发布系统复制到自身 releaseRoot 供下载。
- 生产前端物理目录：/opt/hengjili/releases/marketing-portal-1ee9712，内容与官网合入 main 的 tree 相同。
