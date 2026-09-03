$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

function Test-Command($Name) {
  return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

if (-not (Test-Command "winget")) {
  throw "winget was not found. Install or update App Installer from Microsoft Store."
}

if (-not (Test-Command "node")) {
  winget install --exact --id OpenJS.NodeJS.LTS --silent --accept-source-agreements --accept-package-agreements
}

if (-not (Test-Command "rustup")) {
  winget install --exact --id Rustlang.Rustup --silent --accept-source-agreements --accept-package-agreements
}

$VsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$HasBuildTools = (Test-Path $VsWhere) -and (& $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath)
if (-not $HasBuildTools) {
  winget install --exact --id Microsoft.VisualStudio.2022.BuildTools --silent --accept-source-agreements --accept-package-agreements --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
}

$env:Path = @(
  "$env:ProgramFiles\nodejs",
  $(if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME "bin" }),
  "$env:USERPROFILE\.cargo\bin",
  $env:Path
) | Where-Object { $_ }
$env:Path = $env:Path -join ";"

if (-not (Test-Command "node")) {
  throw "Node.js is installed but PATH is not refreshed. Reconnect SSH and run this script again."
}
if (-not (Test-Command "rustup")) {
  throw "Rustup is installed but PATH is not refreshed. Reconnect SSH and run this script again."
}

rustup default stable-msvc
rustup target add x86_64-pc-windows-msvc
rustup component add rustfmt clippy

npm ci
npm run build
cargo fmt --manifest-path src-tauri\Cargo.toml --check
cargo clippy --manifest-path src-tauri\Cargo.toml -- -D warnings
npm run tauri build -- --target x86_64-pc-windows-msvc --bundles nsis,msi

$BundleRoot = Join-Path $ProjectRoot "target\x86_64-pc-windows-msvc\release\bundle"
$Artifacts = Get-ChildItem $BundleRoot -Recurse -File | Where-Object { $_.Extension -in ".exe", ".msi" }
if (-not $Artifacts) {
  throw "Build finished but no EXE or MSI installer was found."
}

$HashFile = Join-Path $BundleRoot "SHA256SUMS.txt"
$Artifacts | ForEach-Object {
  $Hash = Get-FileHash $_.FullName -Algorithm SHA256
  "$($Hash.Hash.ToLower())  $($_.FullName.Substring($BundleRoot.Length + 1))"
} | Set-Content -Encoding utf8 $HashFile

Write-Host "Windows build completed:" -ForegroundColor Green
$Artifacts | ForEach-Object { Write-Host $_.FullName }
Write-Host $HashFile
