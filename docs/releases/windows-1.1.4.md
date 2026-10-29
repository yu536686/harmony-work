# Windows 1.1.4 正式发布

基线：已发布 RC2，源码检查点 c2bf57e；正式分支 release/windows-1.1.4。

## 发布清单

- [x] 正式元数据与轻量版本门禁同步；锁定依赖不变。
- [x] 前端、原生与打包合同回归。
- [x] 显式构建 1.1.4 内层 NSIS，全量 WPF 离线包及轻量更新包；冻结输入并检查载荷哈希。
- [x] 更新验签及篡改拒绝、COS/CDN 完整回下载。
- [x] 官网创建正式发布记录，完整和轻量入口均走 CDN；逐项验证首页、产品页和直接下载入口。
- [x] Windows 正式更新清单升至 1.1.4，保留测试和 Mac 清单。
- [x] 复核 main、合并并推送 GitHub，记录提交与回退。

沿用用户此前安装器交互免测要求，不操作正在使用的 Admin 客户端；未把构建/载荷校验描述成真实安装验收。

## 发布范围

以 RC2 功能代码为基线，仅切换正式元数据、轻量版本门禁与说明。npm/Cargo 锁文件仅变更本项目版本，不更新依赖；功能代码、Mac 原生模块和资源与 RC2 相同。本次不构建或替换 Mac 安装包。

包含腕表如诗内容看板、营销权限隔离、Jellyfin 流式预览、统一断点上传队列，以及 RC2 的档案毛玻璃弹窗、卡片入场动画、检索和上传阶段进度。

## 构建与完整性

Windows 隔离构建目录：`E:\harmony-work-114-rc1`。正式元数据提交：`7b82b37`。

| 产物 | 字节数 | SHA-256 |
| --- | ---: | --- |
| 全量 `Harmony-Work-1.1.4-x64-setup.exe` | 795217457 | `ad5279ac843de56138e784335d7bb5db4b53955bd78d6f0607c3e9ffe8f1c502` |
| 轻量 `Harmony-Work-1.1.4-x64-light.exe` | 19009451 | `07481d3d3e215b71e7efa6d3179549163c975a7dcac7d5aa75fab89ab071d62e` |

- 显式内层 NSIS：`target\release\bundle\nsis\Harmony Work_1.1.4_x64-setup.exe`，SHA-256 `e0ae97267134d1ef409c5a33b5a6b045fb174e0e03357240c7e5822fd9d3b882`。
- 主程序 ProductVersion 为 `1.1.4`，SHA-256 `9cb9de7b002418971c2afbdab7b620962245001c3d46604eeb028a1bef53d1b1`。
- 全量 WPF 位于 `release\full-114-stable`，轻量包位于 `release\light-114-stable`；离线运行时使用已校验的锁定缓存。全量暂存清单版本与 7 个载荷哈希均通过。
- 轻量包附带 Tauri/minisign 更新签名，验证原文件有效、单字节篡改被拒绝。此签名不是 Windows Authenticode，安装器仍可能显示未知发布者。
- COS 和公开 CDN 对完整文件逐一回下载核对大小及 SHA-256；同名版本文件仅允许复用相同内容，禁止覆盖不同字节。

## 自动验证

- 客户端：68 个 Vitest 文件，415 项通过；Node 合同测试 38 项通过。
- 轻量清单：先观察正式版本门禁失败，再修复并通过 12 项测试。
- 原生：471 项通过、4 项忽略，使用隔离运行时测试环境，不改系统 Homebrew 或用户客户端。
- 门户服务：133 项通过、4 项跳过；下载和发布针对性复核 26 项通过，服务编译成功。
- Windows 优化构建与 NSIS、WPF 全量、轻量生成均成功。现有前端大块提示和 WPF 可空注解警告不阻断构建。
- 安装器真实交互及客户端内实际升级由用户自行验收；未操作 Admin 正在使用的客户端。网页使用页面内容和真实 HTTP 下载链路核对，未将其描述为人工点击验收。

## 官网、CDN 与更新通道

- [官网](https://www.harmonywork.com.cn/workbuddy#download) 的 Windows 版本记录为 `v1.1.4`，发布 ID `3df98503-cf7f-4021-8268-4ed44a3b2069`。
- [全量 CDN](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.4-x64-setup.exe)、[轻量 CDN](https://download.harmonywatch.cn/releases/Harmony-Work-1.1.4-x64-light.exe) 及 `.exe.sig` 已上线。
- 主域名及 `www` 的首页、产品页、版本 API、直接下载 HEAD/GET 和创建下载会话 POST 均核对；完整包统一 307 跳转固定 CDN 域名，保留统计会话，轻量包直接 CDN 下载。Range 返回 206 和正确总大小。
- CDN 改动为门户 `WORKBUDDY_CDN_DOWNLOADS=true` 开关，未部署旧版整个前端，避免覆盖 Mac 下载内容。
- 正式更新清单 `updates/windows/x86_64/stable.json` 为 `1.1.4`，使用原 ETag 条件写入；测试清单保持 `1.1.4-rc2`。本桶检查到的全部其他更新清单逐字节未变，未发布 Mac 更新清单。
- 官网 Mac 下载仍为 `1.1.2 R2`，未改变历史 Windows 版本的不可变下载地址。

## 证据与回退

本机操作记录位于 `release/verification/`（忽略目录）：`windows114-stable-receipt.json`、`114-stable-cdn-verified.json`、`114-stable-public-verification.json`、`114-stable-feeds-before.json`、`114-stable-feed-published.json`。构建及验证日志保留在构建目录和本机 `/tmp/114stable-*.log`。

门户完整包保存于 `/opt/hengjili/marketing-portal/public/downloads/releases/Harmony-Work-1.1.4-x64-setup.exe`，旧 1.1.3 文件与发布记录保留。服务器备份目录为 `/opt/hengjili/backups/windows114-stable/`。

回退时分别处理门户活动发布和正式更新清单：在发布事务锁下归档当前版本、恢复原 `8d1876ec-b505-4367-a91b-5d55c8819c43` 活动记录；仅在核对当前正式清单仍为本次内容后，以 ETag 条件恢复备份的 stable 清单。无需删除已发布的版本文件。CDN 开关可独立撤回，不应回退或覆盖其他新部署模块。

## Git 集成

应用 main 已快进至 `5f28427` 并推送 `yu536686/harmony-work`，包含 RC2 检查点 `c2bf57e` 和正式元数据 `7b82b37`；本记录的收尾提交随后一并推送。主分支检出目录原为干净状态，未覆盖用户未提交修改。

官网下载服务及发布说明为门户仓库 `yu536686/xiaoheng-portal` 的 `1b63ca4`，已推送 main；未将该工作树其他未提交的素材库开发内容混入本次下载服务提交。
