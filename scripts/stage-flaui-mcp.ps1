param(
  [string]$CacheDirectory = (Join-Path $PSScriptRoot "..\installer\payload-cache\automation"),
  [string]$Destination = (Join-Path $PSScriptRoot "..\src-tauri\resources\flaui-mcp")
)

$ErrorActionPreference = "Stop"
$Lock = (Get-Content -Raw (Join-Path $PSScriptRoot "..\installer\automation-lock.json") | ConvertFrom-Json).flauiMcp
$Archive = Join-Path $CacheDirectory $Lock.fileName
New-Item -ItemType Directory -Force -Path $CacheDirectory | Out-Null

if (-not (Test-Path -LiteralPath $Archive)) {
  Invoke-WebRequest -Uri $Lock.url -OutFile $Archive
}
$ActualHash = (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ActualHash -ne $Lock.sha256.ToLowerInvariant()) {
  throw "FlaUI MCP archive SHA-256 mismatch: $Archive"
}

$Staging = Join-Path $CacheDirectory "flaui-mcp-unpacked"
Remove-Item -LiteralPath $Staging -Recurse -Force -ErrorAction SilentlyContinue
Expand-Archive -LiteralPath $Archive -DestinationPath $Staging -Force
$Executable = Get-ChildItem -LiteralPath $Staging -Recurse -File -Filter "FlaUI.Mcp.exe" |
  Select-Object -First 1
if (-not $Executable) { throw "FlaUI.Mcp.exe is missing from the pinned archive." }

Remove-Item -LiteralPath $Destination -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
Copy-Item -Path (Join-Path $Executable.DirectoryName "*") -Destination $Destination -Recurse -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot "..\installer\licenses\FlaUI-MCP.LICENSE") -Destination (Join-Path $Destination "LICENSE.txt") -Force
if (-not (Test-Path -LiteralPath (Join-Path $Destination "FlaUI.Mcp.exe"))) {
  throw "FlaUI MCP resource staging failed."
}
Write-Host "FlaUI MCP $($Lock.version) staged for the Windows installer."
