param(
  [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\release\test")
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Version = (Get-Content -Raw (Join-Path $ProjectRoot "package.json") | ConvertFrom-Json).version
$Project = Join-Path $ProjectRoot "installer\HarmonyWorkUninstaller\HarmonyWorkUninstaller.csproj"
$PublishDirectory = Join-Path $ProjectRoot "installer\.build\uninstaller"

dotnet publish $Project -c Release -r win-x64 -o $PublishDirectory
if ($LASTEXITCODE -ne 0) { throw "Harmony Work uninstaller publish failed." }

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$Published = Join-Path $PublishDirectory "Harmony-Work-Uninstaller.exe"
$Versioned = Join-Path $OutputDirectory "Harmony-Work-$Version-x64-uninstaller.exe"
if (-not (Test-Path -LiteralPath $Published)) { throw "Published uninstaller not found: $Published" }
Copy-Item -LiteralPath $Published -Destination $Versioned -Force
Write-Host "Standalone uninstaller: $Versioned" -ForegroundColor Green
