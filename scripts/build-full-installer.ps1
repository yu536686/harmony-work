param(
  [string]$PayloadCache = (Join-Path $PSScriptRoot "..\installer\payload-cache"),
  [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\release\full"),
  [switch]$SkipDownload,
  [switch]$SkipTauriBuild,
  [string]$InnerInstaller,
  [string]$Version = "1.0.9"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot
if ([string]::IsNullOrWhiteSpace($Version)) {
  throw "The full installer version must be explicit."
}
$PackageVersion = (Get-Content -Raw (Join-Path $ProjectRoot "package.json") | ConvertFrom-Json).version
if ($Version -ne $PackageVersion) {
  throw "Installer version $Version does not match package.json version $PackageVersion."
}

if (-not $SkipDownload) {
  # A cached fetch may not launch a native process, so reset any exit code
  # inherited from the calling PowerShell host before checking this invocation.
  $global:LASTEXITCODE = 0
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

$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot "stage-full-installer-payload.ps1") -InnerInstaller $InnerInstaller -PayloadCache $PayloadCache
if ($LASTEXITCODE -ne 0) { throw "Failed to stage full installer payload." }

$StagedPayload = Join-Path $ProjectRoot "installer\.build\payload"
$PayloadArchive = Join-Path $ProjectRoot "installer\.build\payload.zip"
Remove-Item -LiteralPath $PayloadArchive -Force -ErrorAction SilentlyContinue
[System.IO.Compression.ZipFile]::CreateFromDirectory(
  $StagedPayload,
  $PayloadArchive,
  [System.IO.Compression.CompressionLevel]::Optimal,
  $false)

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
dotnet publish (Join-Path $ProjectRoot "installer\WorkBuddyBootstrapper\WorkBuddyBootstrapper.csproj") `
  -c Release `
  -r win-x64 `
  -p:PayloadDirectory="$(Join-Path $ProjectRoot 'installer\.build\payload')" `
  -p:PayloadArchive="$PayloadArchive" `
  -p:Version="$Version" `
  -o $OutputDirectory
if ($LASTEXITCODE -ne 0) { throw "WPF bootstrapper publish failed." }

$PublishedInstaller = Join-Path $OutputDirectory "Harmony-Work-Setup.exe"
$FinalInstaller = Join-Path $OutputDirectory "Harmony-Work-$Version-x64-setup.exe"
if (-not (Test-Path -LiteralPath $PublishedInstaller)) {
  throw "Published bootstrapper not found: $PublishedInstaller"
}
Copy-Item -LiteralPath $PublishedInstaller -Destination $FinalInstaller -Force
Write-Host "Full offline installer: $FinalInstaller" -ForegroundColor Green

& (Join-Path $PSScriptRoot "build-uninstaller.ps1") -OutputDirectory $OutputDirectory
if ($LASTEXITCODE -ne 0) { throw "Standalone uninstaller publish failed." }
