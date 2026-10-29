# Windows 1.1.4-rc2 测试发布

## 变更

- 腕表如诗的完整档案从页尾改为大尺寸毛玻璃模态弹层；视频与共享档案使用同一入口实现，支持内部滚动、Esc/遮罩关闭、关闭动效与焦点恢复。
- 视频卡片错峰入场、列表/封面骨架；共享档案补充真实规格、标签、状态和来源数量。
- 检索与上传分阶段呈现；未知进度不虚构百分比，传输完成不冒充入库完成。
- 兼容减少动态效果和减少透明度，检查 390/760/1440 三种尺寸、浅深色及键盘操作。
- 无服务端变更，无新增离线运行环境，使用轻量更新。

## 验证

- 前端 68 个测试文件、415 项测试及 Node 合同 38 项通过。
- 前端构建、RC2 轻量包清单/依赖完整性专项通过。
- 原生回归 471 项通过、4 项忽略。本机 Homebrew Node 缺少动态库时的首次运行失败，随后使用隔离测试目录中的有效 Node 24.18.0 重新运行同一 RC2 测试二进制通过，未修改系统 Homebrew 或产品逻辑。
- 真组件 Playwright 夹具覆盖两个档案入口、弹层长内容滚动、关闭与焦点恢复及三尺寸浅深色；不将示例数据视为真实服务验收。
- 沿用用户的安装器交互免测要求；不会操作正在使用的 Admin 客户端。

## 发布状态

2026-10-29：构建、轻量打包、更新签名、篡改拒绝、COS 源站及 CDN 完整回下载全部通过。Windows preview.json 已提升到 1.1.4-rc2，公开 CDN 再次读回验证成功。stable.json 保持 1.1.3，其余更新清单与发布前哈希一致。RC1 的发布基线与证据保留，未覆盖其不可变包。

- 最终包：Harmony-Work-1.1.4-rc2-x64-light.exe，19,010,906 字节。
- SHA-256：c056fb3138fb0957ae37bf12c49ae9e61bd70f19ffc4da230405be91d3afe894。
- CDN：https://download.harmonywatch.cn/releases/testing/Harmony-Work-1.1.4-rc2-x64-light-c056fb3138fb.exe
- Windows 构建位置：`E:\harmony-work-114-rc1\release\light114-rc2-final\`（复用隔离编译缓存，不是 RC1 安装包）。
- 本机：`release/light/Harmony-Work-1.1.4-rc2-x64-light.exe`。
- 构建源码 UI 与版本/锁文件已逐字节比较。原生 release 优化构建耗时 10 分 22 秒。
- 发布证据：`release/verification/114rc2-feeds-before.json`、`114rc2-cdn-verification.json`、`windows114rc2-pack-receipt.json`；发布脚本 `publish-114rc2-preview.py`。
- 仅推送 Windows 测试通道，未更新官网正式下载、Mac 安装包或 Git 主分支。
