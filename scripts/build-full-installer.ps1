param(
  [string]$PayloadCache = (Join-Path $PSScriptRoot "..\installer\payload-cache"),
  [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\release\full"),
  [switch]$SkipDownload,
  [switch]$SkipTauriBuild,
  [string]$InnerInstaller
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

if (-not $SkipDownload) {
  & (Join-Path $PSScriptRoot "fetch-offline-runtime.ps1") -Destination $PayloadCache
  if ($LASTEXITCODE -ne 0) { throw "Failed to download offline runtime payload." }
}

if (-not $InnerInstaller -and -not $SkipTauriBuild) {
  npm run tauri build -- --target x86_64-pc-windows-msvc --bundles nsis
  if ($LASTEXITCODE -ne 0) { throw "Tauri NSIS build failed." }
}

if (-not $InnerInstaller) {
  $TargetDirectories = @(
    (Join-Path $ProjectRoot "target"),
    (Join-Path $ProjectRoot "src-tauri\target")
  ) | Where-Object { Test-Path -LiteralPath $_ }
  $InnerInstaller = $TargetDirectories | ForEach-Object {
    Get-ChildItem -Path $_ -Recurse -Filter "*setup.exe" -ErrorAction SilentlyContinue
  } |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1 -ExpandProperty FullName
}
if (-not $InnerInstaller) { throw "Could not locate the generated NSIS installer." }

& (Join-Path $PSScriptRoot "stage-full-installer-payload.ps1") -InnerInstaller $InnerInstaller -PayloadCache $PayloadCache
if ($LASTEXITCODE -ne 0) { throw "Failed to stage full installer payload." }

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
dotnet publish (Join-Path $ProjectRoot "installer\WorkBuddyBootstrapper\WorkBuddyBootstrapper.csproj") `
  -c Release `
  -r win-x64 `
  -p:PayloadDirectory="$(Join-Path $ProjectRoot 'installer\.build\payload')" `
  -p:Version="1.1.0-alpha.1" `
  -o $OutputDirectory
if ($LASTEXITCODE -ne 0) { throw "WPF bootstrapper publish failed." }

Write-Host "Full offline installer: $(Join-Path $OutputDirectory 'Xiaoheng-WorkBuddy-Setup.exe')" -ForegroundColor Green
