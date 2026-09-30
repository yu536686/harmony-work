# macOS 离线运行时实现计划

> 面向 AI 代理的工作者：在当前 Mac 功能分支执行，保留已有 Harvis 修改；完成每项后更新复选框，不能把编译成功等同于干净 Mac 验收。

**目标：** Apple Silicon macOS 包在无 GitHub、npm、Python 下载访问的首次安装场景中，内置并检测 Git、Node/npm、Python/pip、WeCom CLI，保留 Windows 独立打包链路。

**架构：** 四项运行时都随 `.app/Contents/Resources/resources` 分发，不修改系统 PATH 或全局包。构建机下载并校验锁定载荷，Git 从源码以 `RUNTIME_PREFIX` 构建，Python 使用锁定的 uv 管理版，Node 使用官方完整发行包，WeCom CLI 使用 Apple Silicon 原生包。应用通过统一的 `managed_runtime_executable` 和子进程 PATH 注入调用私有运行时。

**技术栈：** Tauri 2、Rust、Node.js 构建脚本、Git make、uv 管理的 CPython、macOS arm64。

---

### 任务 1：锁定和预置载荷

**文件：** `scripts/stage-macos-offline-runtime.sh`、`scripts/stage-macos-runtime.mjs`、`src-tauri/tauri.macos.conf.json`、`.gitignore`、`src-tauri/licenses/`。

- [x] 测试：检查 Node 完整版、Git 可移动版、Python 与 WeCom 原生程序各自能够在暂存目录运行；校验已下载载荷的 SHA-256。
- [x] 实现：构建只面向 `darwin/arm64`，缓存文件复用前校验哈希，不把在线下载留到用户首次启动。
- [x] 验证：模拟把暂存目录移动后再次运行 Git、Python/pip 和 WeCom；Node/npm 将在 `.app` 打包后验证。

### 任务 2：应用运行时解析与向导

**文件：** `src-tauri/src/process.rs`、`src-tauri/src/commands/platform/system.rs`、`src/components/Bootstrap/index.tsx`。

- [x] 先写失败测试：从 `Contents/MacOS` 找到 `Contents/Resources/resources`，Mac 子进程 PATH 包含私有 bin 目录，环境检测不执行联网 `npx`。
- [x] 实现 Mac 专属路径解析和 PATH，保留 Windows 现有路径映射；向导对随包运行时显示“已内置”。
- [x] 运行 Rust 与前端相关测试，检查 Windows 交叉平台契约。

### 任务 3：打包与验收

**文件：** `scripts/macos-build-contract.test.mjs`、`scripts/platform-isolation-contract.test.mjs`、`README.md`。

- [x] 先写构建契约测试，要求 Mac bundle 包含 node/git/python/wecom，而 Windows bundle 不包含这几个 Mac 资源。
- [x] 执行 Mac 构建，检查 `.app` 和 DMG 内容、可执行权限、运行时版本、Git 命令路径、Python SSL 与 WeCom CLI 启动；DMG 挂载后重复检查。
- [x] 记录包大小、SHA-256 和最终产物路径；干净 Mac 实机由用户验收通过，签名与公证仍需独立处理。

预览包：`release/macos/offline-runtime-preview/Harmony-Work-1.0.7-macos-arm64-offline-preview.dmg`，516,226,106 字节，SHA-256 `3dbf1f3a3fdc6153793a50222e113b7bef9ddb8f63def94649ba728d0dcbe2a6`。已发布到 CDN 的文件名不含 `-preview`，二进制内容相同。
