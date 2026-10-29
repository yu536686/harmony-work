# Windows 1.1.4-rc1 测试发布

## 范围

- 保留现有营销素材库，增加腕表如诗内容看板四视图。
- 资料库使用用户级会话，媒体、HLS 和上传走公司直连 HTTPS，不经腾讯云媒体中转。
- 素材库与看板共用持久上传任务窗口，区分来源，支持分片、校验、暂停、恢复、取消和失败重试。
- 生成签名轻量更新包，仅用于 Windows 测试通道；不更新官网正式下载或 Mac 更新清单。

## 部署

- 29 门户已部署续传模块，systemd 配置 `/etc/systemd/system/hengjili-marketing-portal-api.service.d/harmony114-resumable.conf`。原 index.js/assets.js 备份 `/opt/hengjili/backups/harmony114-resumable/`。
- NAS 私有暂存 `/volume1/Harmony共享/.harmony-upload-staging-114`，真实 uid1026/mode700，无共享 ACL；Jellyfin 用户读取被拒绝。专用 SSH key 仅允许 29 固定 worker 命令，主机密钥固定，凭据不交给 WebView。
- NAS SSH 原配置 `/etc/ssh/sshd_config.pre-harmony114`。DSM 升级后须复核自定义 Match User 和 root 管理的受限公钥入口。
- 231 数据库在新增 client_request_id 前已备份 `/opt/watch-library/backups/harmony114-20261029/library.pre-client-id.db`，旧源码同目录有 pre-client-id 备份。迁移只新增列与索引，不删除旧上传记录。
- 软件转码沿用用户授权设置，原 Jellyfin 配置备份见实施台账。

## 验证

- 前端：67 个 Vitest 文件、411 项测试通过；Node 合同测试 38 项通过；轻量包清单测试 12 项通过。
- Mac 原生：471 项通过，4 项明确忽略；Windows 最新源码 release 编译成功（最后一次优化链接 9 分 12 秒）。
- 231：231 项 Python 测试通过；门户：132 项通过、4 项跳过；Linux 存储/worker：43 项通过。
- 真实用户原生命令通过配置获取、创建、直连传输、NAS 保存、门户索引和共享队列完成状态；真实 HLS 服务探针通过。
- 1440/760/390、浅色/深色的真组件夹具视觉与交互检查通过；它不是 Windows 原生端到端播放验收。
- 原包更新签名通过，单字节篡改拒绝；COS 源站和 CDN 完整回下载一致。
- 用户免除 Windows 安装器交互测试，由用户自行升级测试；未将该项记为通过。Mac 安装包不在本次交付范围。

## 安装包

- 文件：Harmony-Work-1.1.4-rc1-x64-light.exe
- 大小：18,790,379 字节。
- SHA-256：b5738e8f409a12e42a247448712f3de2bdf87ee83ed27f7597ba7ca059b43400。
- Windows：`E:\harmony-work-114-rc1\release\light114-rc1-accepted\`。
- 本机：`release/light/Harmony-Work-1.1.4-rc1-x64-light.exe`。
- CDN：https://download.harmonywatch.cn/releases/testing/Harmony-Work-1.1.4-rc1-x64-light-b5738e8f409a.exe
- 更新签名为客户端更新验签，不等同 Windows Authenticode 代码签名。

## 发布状态

2026-10-29：上传专项复核通过后，最终安装包及签名已上传且完整验证；Windows 测试清单已提升到 1.1.4-rc1，公开 CDN 清单复核通过。正式 Windows 保持 1.1.3，其他更新清单哈希均未变化。执行证据位于 `release/verification/114-*`、`windows114-pack-receipt.json` 和临时测试日志。

首次候选哈希 03c2717b… 因最后两项取消协议修复作废，从未提升测试清单；它不是最终发布包。最终包使用上述带哈希的独立地址，避免覆盖不可变对象。

## 注意

- 上传元数据持久化不保存认证凭据，关闭客户端后不继续传输，重开后需主动恢复。
- 文档/压缩包仅做受限格式检查，不解压，不保证文档内容安全；不支持加密、ZIP64 和超过限制的中央目录。
- 索引持续失败会进入可重试失败态并释放上传槽；重试不会重复创建同一远端任务。
- 本次真实上传测试在当前用户素材目录创建了命名为 harmony114-integration 的小型 PNG 验证文件；未修改现有素材。
- NAS worker 最终固定入口 `/usr/local/libexec/harmony-upload-worker`，Python 模块位于 root 管理的 `/usr/local/libexec/harmony-upload/`；不再从可移动的共享区加载程序。私有暂存目录只存任务数据，升级 DSM 后需复核固定入口、公钥约束及配置持久性。
