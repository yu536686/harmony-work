param(
  [Parameter(Mandatory = $true)][string]$InnerInstaller,
  [Parameter(Mandatory = $true)][string]$Uninstaller,
  [string]$PayloadCache = (Join-Path $PSScriptRoot "..\installer\payload-cache"),
  [string]$Destination = (Join-Path $PSScriptRoot "..\installer\.build\payload")
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$Lock = Get-Content -Raw (Join-Path $ProjectRoot "installer\runtime-lock.json") | ConvertFrom-Json

if (-not (Test-Path -LiteralPath $InnerInstaller)) { throw "Inner NSIS installer not found: $InnerInstaller" }
if (-not (Test-Path -LiteralPath $Uninstaller)) { throw "Standalone uninstaller not found: $Uninstaller" }
if (-not (Test-Path -LiteralPath $PayloadCache)) { throw "Offline runtime cache not found: $PayloadCache" }

Remove-Item -Recurse -Force $Destination -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path (Join-Path $Destination "app"), (Join-Path $Destination "runtime") | Out-Null
$Destination = [System.IO.Path]::GetFullPath($Destination)

Copy-Item -LiteralPath $InnerInstaller -Destination (Join-Path $Destination "app\harmony-work-setup.exe")
Copy-Item -LiteralPath $Uninstaller -Destination (Join-Path $Destination "app\Harmony-Work-Uninstaller.exe")
foreach ($Property in $Lock.components.PSObject.Properties) {
  if ($Property.Name -eq "python") { continue }
  $Source = Join-Path $PayloadCache "runtime\$($Property.Value.fileName)"
  if (-not (Test-Path -LiteralPath $Source)) { throw "Missing pinned runtime: $Source" }
  Copy-Item -LiteralPath $Source -Destination (Join-Path $Destination "runtime\$($Property.Value.fileName)")
}

$Requirements = Join-Path $ProjectRoot "src-tauri\resources\harmony-cut\requirements.txt"
$WheelDirectory = Join-Path $PayloadCache "python-wheels"
if (-not (Test-Path -LiteralPath $WheelDirectory)) { throw "Python wheel cache not found: $WheelDirectory" }
$PythonArchive = Join-Path $PayloadCache "runtime\$($Lock.components.python.fileName)"
if (-not (Test-Path -LiteralPath $PythonArchive)) { throw "Embedded Python runtime not found: $PythonArchive" }
$PythonStage = Join-Path $Destination "python-stage"
try {
  Expand-Archive -LiteralPath $PythonArchive -DestinationPath $PythonStage -Force
  $SitePackages = Join-Path $PythonStage "Lib\site-packages"
  New-Item -ItemType Directory -Force -Path $SitePackages | Out-Null
  [System.IO.File]::WriteAllText((Join-Path $PythonStage "python312._pth"), "python312.zip`n.`nLib/site-packages`nimport site`n")
  & py -3.12 -m pip install --disable-pip-version-check --no-index --find-links $WheelDirectory --target $SitePackages -r $Requirements
  if ($LASTEXITCODE -ne 0) { throw "Failed to vendor Harmony Cut Python dependencies." }
  & (Join-Path $PythonStage "python.exe") -c "import cv2, numpy, starlette, uvicorn, pydantic, imageio_ffmpeg"
  if ($LASTEXITCODE -ne 0) { throw "Isolated Python runtime could not load Harmony Cut dependencies." }
  Compress-Archive -Path (Join-Path $PythonStage "*") -DestinationPath (Join-Path $Destination "runtime\python-runtime.zip") -CompressionLevel Optimal -Force
} finally {
  Remove-Item -Recurse -Force $PythonStage -ErrorAction SilentlyContinue
}

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
