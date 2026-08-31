param(
  [Parameter(Mandatory = $true)][string]$InnerInstaller,
  [string]$PayloadCache = (Join-Path $PSScriptRoot "..\installer\payload-cache"),
  [string]$Destination = (Join-Path $PSScriptRoot "..\installer\.build\payload")
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Lock = Get-Content -Raw (Join-Path $ProjectRoot "installer\runtime-lock.json") | ConvertFrom-Json

if (-not (Test-Path -LiteralPath $InnerInstaller)) { throw "Inner NSIS installer not found: $InnerInstaller" }
if (-not (Test-Path -LiteralPath $PayloadCache)) { throw "Offline runtime cache not found: $PayloadCache" }

Remove-Item -Recurse -Force $Destination -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $Destination "app"), (Join-Path $Destination "runtime") | Out-Null

Copy-Item -LiteralPath $InnerInstaller -Destination (Join-Path $Destination "app\xiaoheng-workbuddy-setup.exe")
foreach ($Property in $Lock.components.PSObject.Properties) {
  $Source = Join-Path $PayloadCache "runtime\$($Property.Value.fileName)"
  if (-not (Test-Path -LiteralPath $Source)) { throw "Missing pinned runtime: $Source" }
  Copy-Item -LiteralPath $Source -Destination (Join-Path $Destination "runtime\$($Property.Value.fileName)")
}

$Requirements = Join-Path $ProjectRoot "src-tauri\resources\harmony-cut\requirements.txt"
Copy-Item -LiteralPath $Requirements -Destination (Join-Path $Destination "runtime\harmony-cut-requirements.txt")
$WheelDirectory = Join-Path $PayloadCache "python-wheels"
if (-not (Test-Path -LiteralPath $WheelDirectory)) { throw "Python wheel cache not found: $WheelDirectory" }
Compress-Archive -Path (Join-Path $WheelDirectory "*") -DestinationPath (Join-Path $Destination "runtime\harmony-cut-wheelhouse.zip") -CompressionLevel Optimal -Force

$Files = Get-ChildItem -LiteralPath $Destination -Recurse -File | ForEach-Object {
  $RelativePath = $_.FullName.Substring($Destination.Length).TrimStart([char[]]@([char]92, [char]47)) -replace '\\', '/'
  [PSCustomObject]@{
    path = $RelativePath
    sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
  }
}
$Version = (Get-Content -Raw (Join-Path $ProjectRoot "package.json") | ConvertFrom-Json).version
[PSCustomObject]@{ version = $Version; files = @($Files) } |
  ConvertTo-Json -Depth 4 |
  Set-Content -LiteralPath (Join-Path $Destination "payload.manifest.json") -Encoding UTF8

Write-Host "Full installer payload staged: $Destination" -ForegroundColor Green
