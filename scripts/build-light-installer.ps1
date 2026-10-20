param([Parameter(Mandatory=$true)][string]$Version,[Parameter(Mandatory=$true)][string]$InstalledPayload,[Parameter(Mandatory=$true)][string]$BaselineInstall,[Parameter(Mandatory=$true)][string]$BaselineManagedRuntime,[string]$CompatibleManagedRuntime='',[string]$OutputDirectory='',[string]$MakeNsis='makensis.exe')
$ErrorActionPreference='Stop'
if($Version -notmatch '^1\.1\.(1|2(?:-rc\.[1-9]\d*)?)$'){throw 'Expected 1.1.1, 1.1.2, or a 1.1.2 release candidate'}
$exe=Join-Path $InstalledPayload 'harmony-work.exe'
$embedded=(Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
if($embedded -ne $Version){throw "Embedded EXE version $embedded differs from requested $Version"}
$root=Split-Path -Parent $PSScriptRoot
if(!$OutputDirectory){$OutputDirectory=Join-Path $root 'release\light'}
$stage=Join-Path $root 'installer\.build\light'
Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage,$OutputDirectory|Out-Null
Copy-Item -LiteralPath $exe -Destination $stage
foreach($relative in @('resources\workbuddy-mcp-host','resources\harmony-cut','resources\artifact-runtime\THIRD_PARTY_NOTICES.txt')){
 $source=Join-Path $InstalledPayload $relative
 if(!(Test-Path -LiteralPath $source)){throw "Missing core payload $relative"}
 $dest=Join-Path $stage $relative
 New-Item -ItemType Directory -Force (Split-Path -Parent $dest)|Out-Null
 Copy-Item -LiteralPath $source -Destination $dest -Recurse
}
# Copy optional owned license directory when present; existing full installations may omit it.
if(Test-Path -LiteralPath (Join-Path $InstalledPayload 'licenses')) { Copy-Item -LiteralPath (Join-Path $InstalledPayload 'licenses') -Destination (Join-Path $stage 'licenses') -Recurse }
$manifestArgs=@((Join-Path $PSScriptRoot 'light-manifest.mjs'),$stage,$BaselineInstall,$BaselineManagedRuntime,$Version,(Join-Path $stage 'payload.manifest.json'),$InstalledPayload)
if($CompatibleManagedRuntime){$manifestArgs+=$CompatibleManagedRuntime}
& node @manifestArgs
if($LASTEXITCODE -ne 0){throw 'Manifest creation failed'}
$out=Join-Path ([IO.Path]::GetFullPath($OutputDirectory)) "Harmony-Work-$Version-x64-light.exe"
Push-Location (Join-Path $root 'installer\light')
try{& $MakeNsis '/INPUTCHARSET' 'UTF8' "/DVERSION=$Version" "/DPAYLOAD=$stage" "/DOUTPUT=$out" 'overlay.nsi';if($LASTEXITCODE -ne 0){throw 'NSIS compilation failed'}}finally{Pop-Location}
Get-FileHash -LiteralPath $out -Algorithm SHA256
