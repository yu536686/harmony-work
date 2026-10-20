# Python 运行环境兼容性修正 r2 — 2026-10-19

用户在 1.1.1 → 1.1.2-rc.1 更新时，预检查报告缺少 python/python312._pth。SSH 读取实际失败日志后确认：用户运行完整 Python 3.12.8，缺少仅嵌入式发行版需要的 python312._pth 与 python312.zip；原清单其余 1649 项匹配，核心依赖可导入。首次验证只覆盖了嵌入式基线，遗漏了完整布局。

## 修正

更新包增加两个精确哈希兼容清单：python-embedded（1678 文件）和 python-standard（6057 文件）。两者共享非 Python 运行环境，并要求解释器和既有 site-packages 核心依赖一致。标准库、DLLs 和根目录原生扩展均列入各自清单；安装时按布局选取清单并逐项校验，继续执行 Python、Node、Git 启动检查。没有简单跳过缺失文件。

只重新打包和签名轻量安装器；主程序、完整安装包、运行环境文件和用户数据均不改动。使用新的 -light-r2.exe CDN 文件名，保留旧不可变安装包，更新 stable/preview feed 与官网轻量包元数据。客户端版本仍为 1.1.1 / 1.1.2-rc.1。已有 1.1.1 无需重装，重新发起更新会读取新下载地址。

## 验证

- 新增回归先失败后修正，Node 12 项测试通过。
- 实际用户账号完整布局预检查退出码 0，74.3 秒；原主程序哈希不变，进程保持运行，版本仍为 1.1.1。
- 隔离测试账号嵌入式布局预检查退出码 0，17.1 秒。
- Windows 验证函数测试：篡改标准 Python _ssl.pyd 预期哈希会被拒绝，缺少匹配布局清单也被拒绝。
- 两个实际轻量包原始签名通过，修改字节后被拒绝；CDN 完整回下载 SHA-256 一致。
- 最终 stable/preview feed、官网 API 均确认指向 -light-r2.exe。

此次未退出或安装到用户当前客户端；用户可在现有界面重新检查更新并确认安装。预检查可能需要约一分钟，实际安装阶段还会再校验一次。

## 下载及 SHA-256

- [Harmony-Work-1.1.1-x64-light-r2.exe](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.1-x64-light-r2.exe)，17504455 字节。

  `bd435dc26ec36504186049bbbd8fbb9ac65c942693b2745dca9f0f7a1cf6ac97`

- [Harmony-Work-1.1.2-rc.1-x64-light-r2.exe](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-rc.1-x64-light-r2.exe)，17506874 字节。

  `6d37e5f3cf09a18034819208e964b5e37b521b9c9512e0883a473eb51973579c`

## 源码和证据

- 桌面分支 feat/windows-light-updater-20261019 修复提交 d932217。
- RC 分支 test/windows-updater-1.1.2-rc.1 修复提交 d217c2d。
- 本地 output/windows-releases-r2/ 包含签名、完整兼容清单、实际用户检查日志、验证脚本和产物元数据。
- 原因与修复均未改动原主工作区、用户 Python 安装或正在运行的客户端。
