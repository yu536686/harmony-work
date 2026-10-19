# Harmony Work Windows 客户端更新发布验证

日期：2026-10-19。已完成开发、Windows 实机覆盖升级、客户端内更新闭环，以及腾讯云 CDN 和官网发布。

## 下载安装包

| 用途 | 版本 | 大小（十进制 MB） | 下载 |
|---|---|---:|---|
| 完整安装包，支持首次安装 | 1.1.1 | 794.12 | [下载](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.1-x64-setup.exe) |
| 正式版轻量手动更新包 | 1.1.1 | 17.07 | [下载](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.1-x64-light.exe) |
| 轻量测试更新包 | 1.1.2-rc.1 | 17.07 | [下载](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.2-rc.1-x64-light.exe) |

官网：[Windows 下载区](https://harmonywork.com.cn/workbuddy#download)。官网完整包和轻量包均为正式版 1.1.1，RC 仅通过预览频道或明确的 RC 文件链接分发。轻量包比完整包缩小约 97.8%。

## 用户操作

1. 首次使用安装 1.1.1 完整包；已有兼容完整安装的用户也可从托盘菜单退出程序，再用 1.1.1 轻量包覆盖更新。
2. 客户端左下角账户菜单 → 关于，可点击“检查更新”。无更新显示“已是最新版本”，有更新弹出内容和“立即更新 / 取消”。
3. 为验证本次 RC，在“关于”勾选“接收测试版本更新”。自动检测或手动检查会发现 1.1.2-rc.1，并在品牌右侧显示可用更新图标。
4. 点击图标打开更新卡片，可“立即安装”或“取消”。取消卡片保留图标；下载和等待阶段也支持取消。
5. 确认安装后，先下载和验证轻量包，完成兼容性检查，等待现有任务完成，再退出、静默覆盖安装和自动重启。

成功检查约每 6 小时执行一次，另加 0–15 分钟随机延迟并持久保存下次时间；首次启动安排在 30–60 秒后。失败按 5 / 15 / 60 分钟及 6 小时循环退避，手动检查有 10 秒防重复限制。后台 15 秒唤醒仅检查是否到期，并非每 15 秒请求服务器。

## 轻量包内容及覆盖边界

实际包含 18 个业务文件：harmony-work.exe、resources/workbuddy-mcp-host 下的主程序、resources/harmony-cut 的 15 个自有文件，以及 THIRD_PARTY_NOTICES.txt。另带兼容性清单和更新脚本。

未打入 Node / Python / Git / WeCom、WebView2、Codex、FlaUI、Playwright 大资源，也不替换原卸载器。安装前核对 189 个复用资源文件和 1651 个固定运行环境文件，并执行 Node、Python 核心模块导入、Git 检查。缺失或不兼容则在覆盖前退出，提示使用完整安装包。该轻量包适用于已验证兼容的已有完整安装，不能用于首次安装。

安装器通过注册表确定实际安装目录；先校验、检查写权限和空间、暂存和备份，再覆盖自有文件。新客户端必须返回匹配随机令牌和版本号的启动确认，并稳定运行 15 秒后提交。普通覆盖/启动故障回滚；中断事务在下次运行安装器时恢复。会话、配置和工作区均不属于覆盖目标。

## 已执行验证

- Windows Rust / MCP 完整回归 296 项通过；最后的交接门禁与缓存修正后，更新器 13 项、独立策略 6 项通过。
- 前端 202 项 Vitest、35 项 Node 契约测试、5 项成果运行时测试通过；正式前端构建通过。
- 轻量安装器 10 项测试通过，包含真实 Git `[.exe` 文件名回归。
- 官网 25 项服务端测试、8 项前端测试及生产构建通过；最终间距修改经生产构建和公网 1440 / 390 像素视口验证，无按钮与提示重叠。
- Windows 实际 1.1.0 → 1.1.1 轻量覆盖成功，自动启动为 1.1.1。
- 正常预检查退出码 0，旧主程序哈希不变；临时移走 Codex 依赖后退出码 1，未覆盖主程序。
- 签名验证使用与 Tauri 更新器相同的 minisign-verify 实现：两个原始轻量包通过，修改一个字节后被拒绝。
- 注入启动版本确认不匹配故障，安装器退出码 1，恢复主程序和 Harmony Cut 脚本并重启旧版，工作区不变；下次正常安装也成功恢复了遗留事务。
- 实际自动检测 RC 后出现品牌更新图标；取消卡片保留图标；下载 49152 字节后取消成功，客户端仍为 1.1.1。
- 实际点击品牌卡片“立即安装”，从真实 CDN 下载 17073958 字节，完成验证、退出和覆盖，自动重启为 1.1.2-rc.1。监测记录中从发起到新版就绪约 35 秒，之后稳定性检查通过，安装器记录 SUCCESS。
- 新版启动后缓存提示已清除，再次检查显示 current，无自身更新提示。
- 原会话、设置 max_iterations=37、MCP 配置、运行环境检测结果及工作区样本均保留。最终覆盖文件 18 / 18、复用组件 189 / 189、运行环境 1651 / 1651 哈希一致。
- 更新后 Codex CLI 0.156.1、Node v22.14.0、Git 2.47.1、Python 核心依赖可运行；FlaUI MCP 12 个工具、Playwright MCP 25 个工具枚举成功。
- 三个安装包已上传腾讯云 CDN，并完整回下载校验 SHA-256；已验证的完整包和正式轻量包 Range 请求返回 206。官网真实下载会话返回 201，并 302 跳转到 CDN 1.1.1 完整包。

验证覆盖了安装/更新和核心组件路径，未穷尽所有账号、外部付费模型和第三方插件组合。基线中 pip 检测已不可用，更新后保持同一状态，本次未改变该运行环境。

## 更新源与兼容说明

- 正式：[stable.json](https://download.harmonywatch.cn/updates/windows/x86_64/stable.json)，version=1.1.1。
- 预览：[preview.json](https://download.harmonywatch.cn/updates/windows/x86_64/preview.json)，version=1.1.2-rc.1。
- 更新器只接受受信任 CDN 的 HTTPS 安装包地址，并先通过嵌入公钥验证签名。
- 当前 feed 不下发可选 pub_date：1.1.1 的日期串展示存在兼容性问题，省略字段后卡片只显示版本和更新内容。本次实际卡片已确认无 Invalid Date。后续若恢复日期展示，需先统一为 RFC3339 或修正前端解析。
- 已下载的更新安装包暂存在用户 updater 目录；本次未实现跨版本缓存的定期清理。

## 分支和部署位置

- 桌面正式源码：[feat/windows-light-updater-20261019](https://github.com/yu536686/harmony-work/tree/feat/windows-light-updater-20261019)，9a9697f8af5f834223ba88cb9b562a7430e72ad9。
- RC 源码：[test/windows-updater-1.1.2-rc.1](https://github.com/yu536686/harmony-work/tree/test/windows-updater-1.1.2-rc.1)，51db748f51fe8368e1064013f7c1b3ed85a83d6d。
- 官网源码：[feat/windows-light-download-20261019](https://github.com/yu536686/xiaoheng-portal/tree/feat/windows-light-download-20261019)，1ee9712。三个分支均已推送，未合并到主分支。
- Windows 源码隔离目录：E:\harmony-work-updater 和 E:\harmony-work-updater-rc；原主工作区的未提交修改未被覆盖。
- 官网新物理目录：192.168.11.29:/opt/hengjili/releases/marketing-portal-1ee9712。systemd 原 b91e476 启动路径现为指向新目录的符号链接；Web 已重启到最终 CSS，API 运行相同的 70042e1 业务代码。持久 .env 与运行时依赖已保留。
- 原官网目录备份：/opt/hengjili/releases/marketing-portal-b91e476.pre-updater-20261019；70042e1 中间版本也保留。
- 完整包服务器副本：/opt/hengjili/marketing-portal/public/downloads/releases/Harmony-Work-1.1.1-x64-setup.exe；活动 release ID 32a500fe-8acf-4051-95cd-02c3f05487f8。
- 腾讯云 Nginx 生效配置：/etc/nginx/sites-enabled/harmony-frp-public-gateway.conf，已定向 1.1.1；备份 /etc/nginx/harmony-frp-public-gateway.backup-updater-20261019。nginx -t 和 reload 通过。
- 完整包沿用已校验的旧版离线运行环境压缩包：runtime-lock 和 requirements 哈希相同，旧 payload.manifest 所列 runtime 文件逐项校验通过。新主程序、NSIS 包和卸载器均按 1.1.1 重新构建。

## 文件 SHA-256

- `Harmony-Work-1.1.1-x64-setup.exe`（794119729 字节）

  `2cd88b8297a1fa81a6a60321f7a7a28c3a37d0806c9895b86869c89f6e721f43`

- `Harmony-Work-1.1.1-x64-light.exe`（17074353 字节）

  `d77fef6715cb301c6210abd83d9b9d02e32240f703bdd3ef1183a1ff620d49e2`

- `Harmony-Work-1.1.2-rc.1-x64-light.exe`（17073958 字节）

  `2c276f60d79eafafb24692ebb0abd55c13fb354e71323d732c86231f72001a30`

## 证据文件

- windows-installer-acceptance.log / windows-preflight-acceptance.log
- windows-update-timeline.ndjson / windows-cancel-download-test.json
- windows-baseline-state.json / windows-stable-state.json / windows-rc-state.json
- windows-rc-mcp-smoke.json / windows-release-artifacts.json
- windows-releases/1.1.1-payload.manifest.json / 1.1.2-rc.1-payload.manifest.json
- windows-about-1.1.1.png / windows-update-card.png / portal-light-final-1440.png / portal-light-final-390.png
- windows-full-reuse-runtime.ps1 / windows-rollback-fault.ps1，记录实际构建和故障验证方法。
