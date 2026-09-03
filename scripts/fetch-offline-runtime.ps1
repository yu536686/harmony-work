param(
  [string]$Destination = (Join-Path $PSScriptRoot "..\installer\payload-cache"),
  [switch]$Force
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$LockPath = Join-Path $ProjectRoot "installer\runtime-lock.json"
$Lock = Get-Content -Raw $LockPath | ConvertFrom-Json

function Get-Sha256([string]$Path) {
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Assert-Hash([string]$Path, [string]$Expected) {
  if ((Get-Sha256 $Path) -ne $Expected.ToLowerInvariant()) {
    Remove-Item -Force $Path -ErrorAction SilentlyContinue
    throw "SHA-256 mismatch: $Path"
  }
}

function Get-PinnedFile($Item, [string]$OutputDirectory) {
  $Path = Join-Path $OutputDirectory $Item.fileName
  if ($Force -or -not (Test-Path -LiteralPath $Path)) {
    Write-Host "Downloading $($Item.fileName)"
    Invoke-WebRequest -Uri $Item.url -OutFile $Path
  }
  Assert-Hash $Path $Item.sha256
  return $Path
}

function Get-PythonWheel($Wheel, [string]$OutputDirectory) {
  $Path = Join-Path $OutputDirectory $Wheel.fileName
  if ($Force -or -not (Test-Path -LiteralPath $Path)) {
    $Metadata = Invoke-RestMethod -Uri "https://pypi.org/pypi/$($Wheel.name)/$($Wheel.version)/json"
    $Release = $Metadata.urls | Where-Object { $_.filename -eq $Wheel.fileName } | Select-Object -First 1
    if (-not $Release) { throw "Pinned wheel not found on PyPI: $($Wheel.fileName)" }
    Write-Host "Downloading $($Wheel.fileName)"
    Invoke-WebRequest -Uri $Release.url -OutFile $Path
  }
  Assert-Hash $Path $Wheel.sha256
}

New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$RuntimeDirectory = Join-Path $Destination "runtime"
$WheelDirectory = Join-Path $Destination "python-wheels"
New-Item -ItemType Directory -Force -Path $RuntimeDirectory, $WheelDirectory | Out-Null

$Lock.components.PSObject.Properties | ForEach-Object {
  Get-PinnedFile $_.Value $RuntimeDirectory | Out-Null
}

foreach ($Wheel in $Lock.pythonWheels) {
  Get-PythonWheel $Wheel $WheelDirectory
}

Write-Host "Offline runtime cache ready: $Destination" -ForegroundColor Green
