# Windows 1.1.3 正式发布执行记录

## 初始状态

- 基于 `feat/windows-1.1.3-rc2` 的 `0e6017b`，远端 `main` 为 `e53dc60`，核对时无超前提交。
- 正式版版本元数据已改为 `1.1.3`，修改尚未提交；新增正式版离线 Python 修复缓存版本门禁及测试，仍使用原有固定哈希校验。
- `npm test`、`npm run build:web` 和提取实际修复源码的正式版本定向测试通过。提取测试不是 Windows 全仓或真实安装验收。
- 没有合并或推送 main；没有构建或上传正式版安装包，没有变更官网或稳定更新清单。

## 2026-10-28 后续进度

- 正式源码提交至 `release/windows-1.1.3`：`d295a32` 版本升级、`409832e` 放行正式轻量包、`7791535` 修复受保护配置文件 ACL；分支已推送，main 未合并。
- Windows 原生 382 项和 MCP 41 项通过；release 程序和隔离轻量包已构建。全量 WPF 包已生成，SHA-256 `572be80656fe4570740c5231df0be0017054a75d1abcbba64975331acdeac898`，存于 `E:\harmony-work-113-stable\release\full-113-stable`。全量包生成前的内层 NSIS 二进制内部文件哈希尚未独立验证。
- 首次轻量回滚发现 Windows PowerShell 5.1 `Set-Acl` 对已保护配置再次写入触发 `SeSecurityPrivilege` 错误。已改为文件/目录 .NET `SetAccessControl`，SMB 独立 DPAPI 及重复 ACL 测试通过；修复包 SHA-256 `32e2a73d60ee30f335d1836e68a3c257c52950c9bfdc7f6b1019772f49a7f58f`，已签名并验证篡改拒绝，签名包在 `release/verification/stable-final/`。先前 pending 事务由新版本自然恢复，隔离升级与故障回滚通过，证据 `release/verification/windows113-stable-retry-acceptance.log`。
- 全量包真实 WPF 首次安装测试尚未完成：当前 SMB 只有 Session 0 SSH，缺少已登录交互桌面；已请求用户登录 SMB。Admin 客户端保持运行，未经确认不可操作。不能把安装引擎单独调用说成 WPF 整包通过。
- 官网 1.1.2 发布 API 当前 HTTP 200；通过此前用户提供的账户已验证 192.168.11.29 SSH 与服务运行，官网管理员发布方式及回退仍待在真实部署中核验。COS/CDN 尚未上传正式包，stable feed 保持 1.1.2，preview 为 1.1.3-rc2。
- `release/light/Harmony-Work-1.1.3-x64-light.exe` 是首次验收失败的旧候选，不得发布；正式候选必须使用 `release/verification/stable-final/` 并校验上述哈希。失败事务及样本已保留。

## 剩余工作

1. 在隔离 Windows 构建目录编译 1.1.3 原生程序及显式指定的正式版 NSIS 内层安装器，检查冻结依赖清单。
2. 构建带离线运行时的完整安装器与轻量包；验证签名、哈希、隔离用户真实升级、回滚与首次安装。
3. 上传 COS，完整回下载校验；更新官网完整包及轻量包信息，再将 Windows 稳定更新通道切换到已验证的轻量包，确认 Mac 和测试通道未意外变化。
4. 再次 fetch、核对 main 没有新提交，提交并合并正式版，推送 GitHub；记录包地址、哈希和验收结果。

避免使用 `build-full-installer.ps1` 的按修改时间自动查找安装器：必须显式指定已验证的 1.1.3 内层 NSIS 包。不要覆盖现有 RC2 安装包或用户正在使用的客户端。

## 2026-10-28 发布结果

- 上述“剩余工作”为发布前的历史状态。用户明确选择自行测试全量安装器，因此未进行交互式 WPF 安装验收；内层 NSIS 文件内容也未独立解包验证，不应将其记为通过。
- 正式源码已合并并推送 `main`，发布代码提交为 `af01437`。Windows 原生测试、MCP 测试及隔离账户轻量升级与故障回滚的结果见上文。
- 全量包 `Harmony-Work-1.1.3-x64-setup.exe`，大小 795024945 字节，SHA-256 `572be80656fe4570740c5231df0be0017054a75d1abcbba64975331acdeac898`；已上传 COS，并完整回下载验证源站和 CDN。
- 轻量包 `Harmony-Work-1.1.3-x64-light.exe`，大小 18769003 字节，SHA-256 `32e2a73d60ee30f335d1836e68a3c257c52950c9bfdc7f6b1019772f49a7f58f`；已上传 COS，完整回下载验证源站和 CDN，签名及篡改拒绝验证通过。
- 官网发布记录 `v1.1.3` 已激活，`v1.1.2` 已归档；全量包与轻量包同时显示。Windows 正式更新清单已更新到 `1.1.3`；测试通道保留 `1.1.3-rc2`。
- 官网下载入口：`https://harmonywork.com.cn/downloads/workbuddy/windows`。CDN 安装包地址分别为 `https://download.harmonywatch.cn/releases/Harmony-Work-1.1.3-x64-setup.exe` 和 `https://download.harmonywatch.cn/releases/Harmony-Work-1.1.3-x64-light.exe`。
