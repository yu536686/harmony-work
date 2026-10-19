# Lightweight overlay installer

Implemented separate NSIS executable; no invocation of previous setup or uninstallers. Existing uninstall.exe and Harmony-Work-Uninstaller.exe are excluded. The actual current-user install directory is discovered through the Harmony Work uninstall registry entry; /D= must match it. Fresh installs and missing or mismatched dependencies fail before staging or replacing app files.

## Payload and compatibility

Owned files only: harmony-work.exe; resources/workbuddy-mcp-host/**; resources/harmony-cut/**; licenses/**; resources/artifact-runtime/THIRD_PARTY_NOTICES.txt. Build validates main EXE ProductVersion exactly matches 1.1.1 or 1.1.2-rc.1. Caller supplies installed-layout payload from the new build. No browser, Codex, FlaUI, Playwright, private Node/Python/Git/WeCom runtime or offline runtime installers enter light payload.

Deterministic payload.manifest.json schemaVersion1 contains version/mainExe/files/dependencies/managedDependencies. Every file entry has path/size/SHA256. Dependencies are exact installed baseline resources (excluding owned paths and offline-installers); managedDependencies are an exact baseline runtime inventory relative to %LOCALAPPDATA%/Harmony/WorkBuddy/runtime. This deliberately rejects changed dependencies. All codex/flaui-mcp/playwright-mcp roots and Node/Python/Git/WeCom executables must exist. WebView2 registered runtime must exist. Baseline 1.1.0 needs no preexisting manifest; direct baseline file hashes support it. When generating the baseline, point to actual installed resource layout and managed runtime, not repository source layout.

## Transaction and health

Single updater mutex; safe relative paths, no traversal or NTFS reparse destinations. Check dependencies and payload before mutation, ask existing app windows to close, wait up to120sec without force killing existing app, fail if still running. Copy payload to same-volume staging, hash stage, back up only files replaced, track newly created files, reverse rollback on any copy/startup failure. Backups persist on failure and log records path. Original uninstall identity stays intact; only DisplayVersion changes after confirmed health. Appdata/workspaces untouched.

Launch new application with HARMONY_UPDATE_HEALTH_FILE and HARMONY_UPDATE_HEALTH_TOKEN. App must atomically write JSON {token,version} after main-window initialization. Independent PowerShell verifies ack within60sec and process survives further15sec. Failure stops only the new process, restores replaced files, removes newly created files and relaunches old version if restoration succeeds. Success removes transaction backups. Log: %LOCALAPPDATA%/Harmony/WorkBuddy/updates/light-installer.log. Manual failures show error/log location; /S logs and returns nonzero. /UPDATE /R accepted; restart and health verification always happen. /D= must be final NSIS argument.

## Tests and Windows build

`node --test scripts/light-manifest.test.mjs`:4 passed on macOS. Checks inclusion/exclusion, Windows unsafe paths, deterministic inventories, byte hashes, missing dependencies, invalid versions, transaction ordering and rollback/health contract. NSIS and PowerShell are unavailable on this Mac; executable compilation and live Windows success/failure/rollback tests remain required. Static source checks do not establish runtime safety. Abrupt OS/process termination recovery is not automatic; retained transaction backup supports manual recovery.

Windows prerequisites: Node, Windows PowerShell5.1 (built-in), NSIS3 with standard nsExec plugin. NSIS path normally `C:\Program Files (x86)\NSIS\makensis.exe` or Tauri cache makensis. No .NET/pwsh runtime needed on target. Command:

```powershell
.\scripts\build-light-installer.ps1 -Version '1.1.1' `
 -InstalledPayload 'C:\build\installed-layout-1.1.1' `
 -BaselineInstall 'C:\baseline\Harmony Work' `
 -BaselineManagedRuntime 'C:\baseline\runtime' `
 -MakeNsis 'C:\Program Files (x86)\NSIS\makensis.exe'
```

Output release/light/Harmony-Work-1.1.1-x64-light.exe. Repeat for1.1.2-rc.1 with matching EXE. Sign resulting installer using Tauri updater signing before publication. Keep same exact baseline dependency inventory for compatible versions. Test install success, dependency corruption rejection, app still running timeout, payload copy fault rollback, no-health rollback, uninstaller hashes and workspace preservation before CDN manifest publication.
