# 腕表如诗内容看板 1.1.4-rc1 实现计划

> 执行方式：当前会话使用 subagent-driven-development 分工，先失败测试，再实现和审查；台账逐项更新。用户已确认设计原型。

目标：保留现有素材库，接入 231 看板全部现有功能，并提供安全登录、流式媒体、可恢复上传。

架构：React 客户端复用现有导航与 UI；独立 Rust 命令限定请求类型，保管服务会话与磁盘任务；231 FastAPI 保持业务服务为唯一事实来源。媒体与上传走公司 HTTPS 入口，不经腾讯云流量代理。不得把管理员密钥或门户登录凭据交给 WebView。

基线：main b2c62a2；工作树 /Volumes/SSD/harmony-work-windows-1.1.3-rc1；分支 feat/watch-dashboard-1.1.4-rc1。腕表服务源码 /Users/yutianqi/Documents/小亨workbuddy/watch-library-worktree，部署 /opt/watch-library，SSH 192.168.11.231。现有素材、生产客户端和稳定更新通道不变。

## Task 1：版本、接口与安全适配

- [x] 前端及原生基线测试，记录已有失败。
- [x] 统一版本元数据为 1.1.4-rc1，不批量改历史文档。
- [x] 新增 services/tauri/watchLibrary.ts：类型化请求和数据，不接受任意 URL。
- [x] 新增 commands/watch_library.rs，并注册命令；Rust 只连接固定受信 HTTPS 域名，禁止重定向泄露 Cookie。
- [x] 请求使用当前门户身份与 generation；退出或换账号后不接受旧请求/上传结果，不在前端持久化凭据。
- [x] 231 新增桌面登录交换及受限媒体接口；源代码先测试，部署前备份。只传当前用户的门户会话给受信服务换取用户会话，不持久化新增长期密码。

测试：拒绝任意路径、重定向和跨账号回填；匿名/标准用户拒绝；错误不包含凭据。运行 cargo test 与 Python unittest 对应测试。

## Task 2：客户端四视图

- [x] 创建 components/WatchLibrary/，复用原型布局但仅请求真实接口。
- [x] 已拍摄内容：品牌、系列、关键词、分页、详情、管理员编辑、封面与媒体预览。
- [x] 检索：文字和图片上传，8 MB 客户端门禁，服务端任务查询、失败/警告/来源/参考图/匹配素材。
- [x] 共享档案：查询、详情、证据、关联素材。
- [x] 上传视图：服务端配置、个人历史、手动分析、状态显示。
- [x] 集成 App、营销导航、两端原生菜单映射；保持原素材库。
- [x] 测试无权限、筛选、任务终态、卸载轮询、换账号清理与窄窗口。

## Task 3：媒体能力

- [x] 231 从服务器侧关联 Jellyfin 条目并获取播放信息，按客户端支持选择直放或 HLS；不得把 Jellyfin 管理密钥发给客户端。
- [x] 媒体票据绑定用户和资源、过期与可撤销；HLS 清单/分片及 Range 同样鉴权。
- [x] 未关联素材经合法路径与权限校验后用流式文件响应；封面使用受控图片地址。
- [x] 下载/会话附件为用户显式动作，限制写入当前工作区；预览不自动缓存完整文件。
- [x] 保留现有素材库可用路径；图片优化为按需获取，不将网页图片 URL 当作视频流。
- [ ] 两端及外网验证实际流、seek、权限、转码停止；无法验证者明确标记，不记为完成。

## Task 4：上传任务窗口与恢复

- [x] Rust 分片读取本地文件而非整文件入内存；复用 231 上传接口、偏移和 SHA-256。
- [x] 任务以用户、目标、路径和源文件身份隔离，磁盘保存任务元数据但不存凭据。
- [x] 最大两并发；进度事件/轮询，暂停、恢复、取消、重试和应用重启恢复；服务端保存确认后完成。
- [x] 全局上传窗口跨页面存在，标注来源；现有素材库上传采用相同队列但保留原目的地和语义，不误送腕表目录。
- [x] 客户端关闭不承诺继续上传，任务过期、文件变化、失权必须明确报错。

## Task 5：验收与交付

- [x] 跑 npm test、build:web、原生检查及服务端专项测试。
- [x] Playwright 真组件页面，桌面/窄窗口截图及交互检查，不能把演示原型测试当作生产验收。
- [ ] 两端编译及实际服务链路验证；发布包与预览通道需另行记录，不改稳定通道。
- [x] 更新台账：每项测试、实际部署、缺口、安装包路径、提交和后续动作。

## 持久化执行台账

- 2026-10-28：用户确认原型并授权开发；独立功能分支已创建。原型仅用于设计，不能作为服务能力完成证据。
- 客户端与服务端分别在 `feat/watch-dashboard-1.1.4-rc1`、`feat/harmony-desktop-1.1.4-rc1` 分支，尚未提交或部署。客户端已有四视图、Rust 请求与上传草稿、版本元数据；231 新增桌面会话及原文件 Range 媒体票据草稿。所有改动均未做正式安全审查。
- 中断恢复后验证：客户端 `npm run build:web` 成功（有 Vite 分包提示）；看板 Vitest 15 项通过；安装器版本锁测试 13 项通过；231 全量 unittest 217 项通过；Mac `npm run test:native` 457 项通过、2 项忽略（详见 `/tmp/harmony114-native-resume.log`）。这些不证明真实端到端功能可用。
- 阻塞交付的缺口：Jellyfin 转码/HLS 及授权分片未实现；现有素材库与看板尚未共用上传队列；桌面单点登录仍采用服务端验证门户凭据的草稿，需要复核与确认授权交换方案；断点续传的并发、账号切换、取消、崩溃恢复与原文件改动需要真实集成测试和安全审查；媒体在真实 WebView 的 CSP、凭据、长时间续播以及外网链路待验证；Windows 编译/两端真实交互及部署验收均未进行。
- 未发布、未更改生产服务或稳定更新通道。下次从上述缺口和完整代码审查继续；不要把已编译误认作完成或部署。保存用户数据和工作树，不覆盖既有生产或其他未提交改动。
- 2026-10-29：核对了真实 Jellyfin 10.10.7：授权下主清单、子清单和 16 个分片可生成；本地加入受限 HLS 代理、跨域媒体票据及 Windows hls.js 支持，模拟 HLS、Range 和撤权测试通过；尚未在真实 Tauri 客户端验收。修复上传使用固定账号会话、服务器保存前完整 SHA 校验、已保存但未入库不标记完成；尚需真实断网续传/账号切换并发和取消恢复测试。Jellyfin 票据地址必须在 29、231 两层 nginx 关闭敏感路径访问日志；目前两个节点未配置，不得发布。
- 本地服务端与客户端仍未部署。29 到 231 现有反向代理为内网明文 HTTP，桌面单点登录会传递门户凭据，需先升级该链路的加密及验证、审核日志与票据；不能以公司内网作为忽略凭据保护的理由。231 SSH key 可登录但缺少 sudo，已请求用户提供授权；等待期间继续开发，不在生产上使用不安全的临时配置。
- 全局上传窗口目前仅支持腕表看板。原素材库仍为 25MB 内存上传，无断点续传；不能宣称两处已共用队列。需独立设计并测试现有素材库的服务端断点协议、权限隔离与冲突处理，或明确产品范围后再实现。
- 2026-10-29 后续：用户提供 231 管理授权，已将 29 到 231 的链路切换为 `https://watch-library.internal:8444` 并校验证书，231 防火墙仅允许 29 连接此端口；公网 `/health` 通过，其他本机连接 8444 超时。两端 nginx 对 `/api/desktop/hls/` 和 `/api/desktop/media/` 禁用访问日志，探针不出现在两端日志。原配置备份：231 `/etc/nginx/sites-available/watch-library.conf.pre-114` 和 `/etc/nftables.conf.pre-114`；29 `/etc/nginx/sites-available/watch-library-public.conf.pre-114`。证书和信任配置存于 231 `/etc/watch-library/tls/`、29 `/etc/nginx/watch-library/upstream-ca.pem`，证书有效期 825 天，届时需协调轮换。服务业务代码尚未部署，生产仍为旧版。
- HLS、上传安全与视觉验证：增加不透明媒体票据、权限撤销检查、HLS 受限代理、客户端授权续期和 HLS 播放依赖；现场验证 Jellyfin 主清单、子清单和 16 个分片，但未验证真实 Tauri 客户端。上传绑定单账号快照、增加存储端完整 SHA 校验、要求成功入库后才完成；仍需真实并发/中断/重启验收。生产写入端 `/opt/hengjili/marketing-portal/.watch-library-upload/writer.py` 也需安全更新并核验版本，否则服务端新增 SHA 校验不会在生产 NAS 生效。全局队列和现有素材库断点上传仍未完成。
- 2026-10-29 更新：用户同意软件转码，已备份 Jellyfin 设置到 231 `/opt/watch-library/backups/harmony114-20261029/jellyfin-encoding.json` 并由 `qsv` 切换为 `none`。原 QSV 失败为 VAAPI 初始化错误。独立 TestClient 与真实 Jellyfin 验证了主清单、媒体清单、视频分片和撤权后的 401；CPU 转码容量、两端真实 WebView 与公网播放尚待验收。
- 231 已备份原 `app.py`、`auth.py`、`uploads.py` 至同一备份目录，部署新版并重启 `watch-library`；公网 `/health` 返回 200，新桌面登录及 HLS 无凭据请求返回 401。29 NAS 写入端 `writer.py` 已备份至 `/opt/hengjili/marketing-portal/.watch-library-upload/backups/harmony114-20261029/writer.py` 并更新，线上文件 SHA-256 与本地 `b7097b1c91226d4e3bc5bd831aa416d76650dabfe18d72c1b642b44386cbe53f` 一致。全链路用户账号、上传和运行时回归尚未确认，需继续测试；不得把服务健康检查当作验收通过。
- 用户要求 Windows 包由用户自行测试，完成后仅推送预览通道。Windows 隔离副本 `E:\harmony-work-114-rc1` 已完成 `npm run build:web`，Windows 原生测试 385 项通过、2 项忽略；本次心跳继续执行 `npm run build` 准备完整编译。该副本为 02:03 时点复制，正式打包前必须与本工作树再次比对同步，不能把旧副本作为最终版。
- 现有营销素材库续传在独立官网仓库 `build/harmony-portal-release-1.1.2` 开发中；隔离存储模块有本地及 NAS 基础测试，但 HTTP 授权、索引、配额与端到端尚未完成，不可提前上线。
- 04:20 本会话自动继续：公网资料库 `/health` 已确认 200。Windows `npm run build` 首次因 FlaUI 离线包下载失败而中断；已从上游下载并核对锁定 SHA-256，再次构建前端与运行时 staging 成功，`npx tauri build --no-bundle` 正在独立目录编译。此时只是候选可执行程序，不能推送测试通道；须等素材库续传真实接入、代码审查、最新源码同步、打包与签名校验完成。用户免除安装后交互测试。
- 官网续传模块进度：Python 暂存、不可覆盖发布及恢复、Node/Python 子进程适配、HTTP 路由工厂有隔离测试；尚未接入真实认证、目标/索引和服务发布。隔离实验显示 CIFS 硬链接虽能创建但读取返回 EINVAL，使用同目录 `renameat2(RENAME_NOREPLACE)` 候选；目录信任边界仍需解决，禁止未经验证直接部署。
- 素材库续传试验模块已补 MP4/MOV/M4A、WebP、WAV、MP3 的文件头识别及测试；本机 17 项通过、6 项 Linux 专项跳过。签名检查不等于完整文件有效性或恶意内容检测，仍未挂载生产路由。客户端 Windows 构建副本在本轮开始后已落后至少一个 `watch_library.rs` 会话修复；必须重新同步后再构建最终更新包。
- 04:20 自动继续期间 Windows `npx tauri build --no-bundle` 候选 release 编译成功（耗时 10 分 58 秒），位于 `E:\harmony-work-114-rc1\target\release\harmony-work.exe`；尚未形成安装包，该候选代码不是最新，不可发布。测试通道未改动；原素材库续传权限/路径与索引接入尚未完成。
- 后续进度：231 媒体票据增加源地址绑定并部署后，使用当前登录账户走真实公网链路的 Rust 媒体流探针通过；不等同 Windows WebView、seek 或长期播放验收。官网素材续传路由已在源码以显式开关接入现有授权、目标规则及索引，但未启用或部署；客户端统一上传队列仍未接入，Linux/NAS 生产信任边界和完整端到端尚需验证。Windows 更新后的候选重新编译中；编译成功也不能作为发布完成的依据。
- 官网素材续传的实际 Linux 临时目录测试：29 主机 `/tmp/harmony114-verify.hQxSRr5y` 运行 Python 存储及 worker 测试共 21 项通过（含 Linux renameat2 测试）；官网 Node 服务测试 123 项通过、3 项跳过，服务端构建通过。上述测试未覆盖生产 NAS 权限、真实会话请求及客户端恢复，不代表已可上线。
- Windows 候选再次编译通过（release 8 分 22 秒，日志 `E:\harmony-work-114-rc1\tauri-build-updated.log`）；当前客户端源码比其多一个仅供本机人工运行的 ignored 线上探针。该二进制尚不包含未完成的素材库统一上传功能，不能制作或发布为最终候选。正式及测试更新通道均未更改。
- 继续执行：官网素材上传完成后长期占用暂存配额的问题已在 Linux 复现并修正；29 临时目录 22 项 Python 测试通过。上传窗口增加素材库/腕表来源区分，素材库入口改为请求统一原生上传任务并提供打开任务按钮；React 定向测试 17 项通过，但对应原生命令仍在实现，不能发布。生产 `ASSET_ROOT=/mnt/harmony`，CIFS 以共享账户挂载，文件系统权限显示统一映射，不足以证明 NAS 上其他 SMB 用户不能更改上传祖先目录或暂存；启用前仍需解决此信任边界及测试真实端到端。
- 本轮：原生营销素材任务与共享队列已接入；客户端及服务端增加创建幂等标识，服务端支持取消幂等和释放已完成任务的配额；29 Linux 临时环境 Python 测试 26 项通过。官网续传尚未部署，需完整审查和端到端测试。用户提供 NAS SSH 后已成功登录 192.168.11.33，只读检查 `/volume1/Harmony共享` 的 Synology ACL 发现多个用户有写权限；尚未修改 NAS 权限或创建生产暂存目录。需要安全隔离暂存、确认索引与 NAS 并发写入的信任边界，然后再编译并仅推送测试通道。
- NAS 私有暂存与固定 worker 已实机打通：`/volume1/Harmony共享/.harmony-upload-staging-114` 属于 uid1026、mode700、无继承共享 ACL，Jellyfin 用户读取测试被拒绝。目录下 runtime/ 为 worker 源码；29 使用专用 `/home/yutianqi/.ssh/harmony_upload_nas` 密钥及固定 known_hosts。NAS `/etc/ssh/harmony-upload-keys/yutianqi` 仅允许来源 192.168.11.29，restrict + forced-command，不能运行普通命令。原 SSH 配置备份 `/etc/ssh/sshd_config.pre-harmony114`；新增 Match User 仅增加 root 管理的公钥文件入口，保持 StrictModes，不修改 home 和原共享 ACL。DSM 升级后应复核配置是否保留。
- 真实 29→NAS worker 探针通过：创建幂等重试、分片、偏移状态、SHA 校验、同设备原子发布与发布后恢复；隔离测试文件仅在私有 integration-target 内，任务 ca5cfdcb3ea34e8e9647a709c9243cfd。探针 `/tmp/harmony114-nas-probe.py`（29）。原生上传队列专项 9 项通过。新增 Node SSH transport 仍需在 29 与门户认证/索引联调，官网续传功能开关未启用，客户端未重新打包或推送。
- Node 适配层到真实 NAS 的独立探针也已通过：创建幂等、取消幂等、状态恢复，任务 02ad8f8af711a7b88be11e932f04eae7；29 `/tmp/harmony114-node-nas-probe.mjs`，调用实际编译的 ResumableWorkerClient。门户服务端重新构建成功；已从生产取回源码比较，index.ts 差异仅为本次续传接入。下一步为生产模块备份/发布、配置 SSH transport、认证及索引端到端、完整代码审查、同步 Windows 并最终打包；不得把私有探针成功当作门户完整路径验收。
- 最终交付：门户续传已部署并启用 SSH transport；原生命令使用真实登录身份完成公网创建/传输/NAS 校验/门户入库/共享队列终态，HLS 真实身份探针再次通过。43 项 Linux 存储测试、132 项门户测试（4 跳过）、231 项看板测试、471 项 Mac 原生测试（4 忽略）通过；前端 411 项和 Node 合同 38 项通过，轻量清单 12 项通过。六项上传创建/取消/去重/配额/索引等待问题均修复并专项复核通过。
- NAS 最终 worker 程序已从共享区移到 root 管理的 `/usr/local/libexec/harmony-upload/`，固定入口 `/usr/local/libexec/harmony-upload-worker`；受限公钥只允许该入口且忽略用户 Python 环境。私有暂存保持 uid1026/mode700。Node→NAS 实机探针再次通过；未放宽原共享或 home 权限。
- 最终 Windows 轻量包 `E:\harmony-work-114-rc1\release\light114-rc1-accepted\Harmony-Work-1.1.4-rc1-x64-light.exe`，18,790,379 字节，SHA-256 b5738e8f409a12e42a247448712f3de2bdf87ee83ed27f7597ba7ca059b43400。客户端源码与最终构建目录关键上传文件已逐字节比对。更新验签及单字节篡改拒绝通过，COS/CDN 完整回下载通过。
- 已提升 Windows preview.json 到 1.1.4-rc1，公开 CDN 再次确认；stable.json 保持 1.1.3，其他更新清单哈希未变。最终包地址：https://download.harmonywatch.cn/releases/testing/Harmony-Work-1.1.4-rc1-x64-light-b5738e8f409a.exe 。首次候选未提升，禁止误用。源码保留功能分支，未合并 main 或改官网正式下载。
- 验证边界：按用户要求未进行 Windows 安装器交互验收；两个平台原生 WebView 的长时间播放、seek 与断网完整人工体验仍由测试版使用验证，不将组件夹具或原生 HTTP 探针冒充该验收。本轮不生成 Mac 安装包。
