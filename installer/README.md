# 全量离线安装器

`WorkBuddyBootstrapper` 是 Windows x64 的 WPF 外层安装器。它提供品牌化安装界面、安装目录选择、快捷方式选项、载荷 SHA-256 校验和私有运行时部署；现有 Tauri NSIS 安装包继续作为内层应用安装器。

## 打包内容

- Tauri NSIS 应用安装包，包含离线 WebView2。
- 私有 Node.js 22.14.0、Python 3.12.8、PortableGit 2.47.1。
- `@wecom/cli` 0.1.9 及 Windows x64 平台二进制。
- Harmony Cut 所需的离线 Python wheelhouse。

运行时部署到 `%LOCALAPPDATA%\Harmony\WorkBuddy\runtime`。WorkBuddy 子进程会优先使用该目录，因此不会修改用户系统 PATH 或替换既有开发环境。

企业微信 CLI 的二进制可离线部署，但 `wecom-cli auth init` 仍必须由当前 Windows 用户完成官方授权。AI 密钥、WorkBuddy 账户及机器人凭据同样不会被安装器预置。

## 构建

在 Windows 上执行：

```powershell
./scripts/fetch-offline-runtime.ps1
./scripts/build-full-installer.ps1 -SkipDownload
```

最终文件为 `release\full\Harmony-Work-<version>-x64-setup.exe`。`runtime-lock.json` 固定每个第三方载荷的版本与 SHA-256；缓存和最终载荷均被 `.gitignore` 排除，不提交到仓库。
