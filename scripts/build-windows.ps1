param(
  [switch]$CleanInstall
)
$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ProjectRoot

function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter(Mandatory = $true)][string[]]$Arguments
  )
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$FilePath $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
  }
}

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  throw "Node.js 22 or newer is required."
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  throw "Rust stable MSVC toolchain is required."
}

if ($CleanInstall) {
  Invoke-Checked "npm" @("ci")
} else {
  Invoke-Checked "npm" @("install")
}
Invoke-Checked "npm" @("run", "build")
Invoke-Checked "npm" @("run", "tauri", "--", "build", "--target", "x86_64-pc-windows-msvc", "--bundles", "nsis")

Write-Host "Windows NSIS installer is in target/x86_64-pc-windows-msvc/release/bundle/nsis/" -ForegroundColor Green
