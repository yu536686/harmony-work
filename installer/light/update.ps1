param([Parameter(Mandatory=$true)][string]$Payload,[string]$InstallDirectory,[switch]$Silent,[switch]$Check,[switch]$Automatic)
$ErrorActionPreference='Stop'
$logRoot=if($Check){Join-Path $env:TEMP 'HarmonyWorkUpdateCheck'}else{Join-Path $env:LOCALAPPDATA 'Harmony\WorkBuddy\updates'}
New-Item -ItemType Directory -Force $logRoot | Out-Null
$log=Join-Path $logRoot 'light-installer.log'
function Log($text) { Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) $text"; Write-Host $text }
function SafeJoin($root,$relative) {
 if($relative -notmatch '^[a-zA-Z0-9_. /@+\[\]()-]+$' -or $relative.StartsWith('/') -or @($relative.Split('/') | Where-Object { !$_ -or $_ -eq '.' -or $_ -eq '..' -or $_.EndsWith('.') -or $_.EndsWith(' ') }).Count) { throw "Unsafe manifest path: $relative" }
 $p=Join-Path $root ($relative.Replace('/','\'))
 $cursor=$p
 while($cursor -and $cursor.Length -ge $root.Length) { if((Test-Path -LiteralPath $cursor) -and ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Reparse point forbidden: $cursor" }; $cursor=Split-Path -Parent $cursor }
 return $p
}
function Verify($root,$items) { foreach($f in $items) { $p=SafeJoin $root $f.path; if(!(Test-Path -LiteralPath $p -PathType Leaf) -or (Get-Item -LiteralPath $p).Length -ne $f.size -or (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash -ne $f.sha256) { throw "Missing or incompatible dependency/payload: $($f.path). Use the full offline installer." } } }
function VerifyManagedRuntime($root,$manifest) {
 if(!$manifest.managedRuntimeProfiles){Verify $root $manifest.managedDependencies;return}
 $id=if((Test-Path -LiteralPath (Join-Path $root 'python/python312._pth')) -and (Test-Path -LiteralPath (Join-Path $root 'python/python312.zip'))){'python-embedded'}else{'python-standard'}
 $profiles=@($manifest.managedRuntimeProfiles|Where-Object {$_.id -eq $id})
 if($profiles.Count -ne 1 -or !@($profiles[0].files).Count){throw "Unsupported managed runtime layout: $id. Use the full offline installer."}
 Verify $root $profiles[0].files
 Log "Runtime compatibility verified profile=$id files=$(@($profiles[0].files).Count)"
}
function SmokeRuntime($root) {
 $checks=@(
  @{path='node/node.exe';args=@('--version')},
  @{path='python/python.exe';args=@('-I','-B','-c','"import cv2,numpy,starlette,uvicorn,pydantic,imageio_ffmpeg; import ssl,sqlite3,zipfile,asyncio"')},
  @{path='git/bin/git.exe';args=@('--version')}
 )
 foreach($check in $checks) {
  $process=$null
  try {
   $process=Start-Process -FilePath (SafeJoin $root $check.path) -ArgumentList $check.args -WindowStyle Hidden -PassThru
   if(!$process.WaitForExit(45000)){ $process.Kill();throw "Runtime preflight timed out: $($check.path)" }
   $process.WaitForExit()
   if($process.ExitCode -ne 0){throw "Runtime preflight failed: $($check.path), exit $($process.ExitCode). Repair with the full offline installer."}
  } finally {if($process){$process.Dispose()}}
 }
}
function ProbeInstallWrite($root) {
 $probe=SafeJoin $root ('.harmony-write-probe-'+[guid]::NewGuid().ToString('N'))
 $stream=$null
 try {
  $stream=[IO.File]::Open($probe,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
  $stream.WriteByte(1);$stream.Flush($true);$stream.Dispose();$stream=$null
  Remove-Item -LiteralPath $probe -Force
 } catch {throw "Installation directory is not writable: $root. $($_.Exception.Message)"}
 finally {if($stream){$stream.Dispose()};if(Test-Path -LiteralPath $probe){Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue}}
}
function GetRunningProduct($exe) { return @(Get-Process harmony-work -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }) }
function WaitForProductExit($exe,[int]$timeoutSeconds=120) {
 $deadline=[DateTime]::UtcNow.AddSeconds($timeoutSeconds)
 $announced=$false
 while(@(GetRunningProduct $exe).Count) {
  if(!$announced){Log 'Waiting for Harmony Work to exit. Do not reopen the client until the update completes.';$announced=$true}
  if([DateTime]::UtcNow -ge $deadline){throw 'Harmony Work is still running. Choose Exit from the tray menu and retry.'}
  Start-Sleep -Milliseconds 250
 }
}
function WaitForInstallAccess($path,[int]$timeoutSeconds=20) {
 if(!(Test-Path -LiteralPath $path)){return}
 $deadline=[DateTime]::UtcNow.AddSeconds($timeoutSeconds)
 do {
  $handle=$null
  try {$handle=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None);return}
  catch {if([DateTime]::UtcNow -ge $deadline){throw "Update target is still in use or not writable: $path. Close Harmony Work and retry. $($_.Exception.Message)"};Start-Sleep -Milliseconds 250}
  finally {if($handle){$handle.Dispose()}}
 }while($true)
}
function OwnedPath($p) { return $p -eq 'harmony-work.exe' -or $p -match '^(resources/(workbuddy-mcp-host|harmony-cut)/|licenses/|resources/artifact-runtime/THIRD_PARTY_NOTICES.txt$)' }
function WriteJournal($state) {
 $journal=@{schemaVersion=1;installDirectory=$InstallDirectory;previousVersion=$key.DisplayVersion;state=$state;changes=@($changed | ForEach-Object { @{path=$_.path;existed=$_.existed;sha256=$_.sha256;size=$_.size} })}
 $tmp=Join-Path $transaction 'journal.tmp'
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $tmp -Encoding UTF8
 Move-Item -LiteralPath $tmp -Destination (Join-Path $transaction 'journal.json') -Force
}
function RecoverTransactions($root,$registryKey) {
 foreach($dir in @(Get-ChildItem -LiteralPath $root -Directory -Filter '.harmony-update-*')) {
  $null=SafeJoin $root $dir.Name
  $journalPath=Join-Path $dir.FullName 'journal.json'
  if(!(Test-Path -LiteralPath $journalPath)){continue}
  $j=Get-Content -Raw -LiteralPath $journalPath|ConvertFrom-Json
  if($j.schemaVersion -ne 1 -or $j.installDirectory -ne $root){throw "Invalid interrupted update journal: $journalPath"}
  if($j.state -ne 'committed') {
   $entries=@($j.changes);[array]::Reverse($entries)
   foreach($entry in $entries) {
    if(!(OwnedPath $entry.path)){throw 'Unsafe interrupted update journal destination'}
    $target=SafeJoin $root $entry.path
    if($entry.existed){Verify (Join-Path $dir.FullName 'backup') @($entry);Copy-Item -LiteralPath (SafeJoin (Join-Path $dir.FullName 'backup') $entry.path) -Destination $target -Force}
    elseif(Test-Path -LiteralPath $target){Remove-Item -LiteralPath $target -Force}
   }
   Set-ItemProperty -LiteralPath $registryKey.PSPath -Name DisplayVersion -Value $j.previousVersion
   Log "RECOVERED interrupted transaction $($dir.FullName)"
  }
  Remove-Item -LiteralPath $dir.FullName -Recurse -Force
 }
}
$changed=@(); $transaction=$null; $child=$null; $mutex=$null; $originalExe=$null; $canRestart=$false; $recoveryFailed=$false
try {
 if(![Environment]::Is64BitProcess){throw 'The light updater requires 64-bit PowerShell. Run the signed lightweight installer instead of a 32-bit script host.'}
 $mutex=New-Object Threading.Mutex($false,'Local\HarmonyWorkLightUpdate'); if(!$mutex.WaitOne(0)){throw 'Another update is running'}
 Log "START check=$Check automatic=$Automatic architecture=x64"
 $keys=@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' | Get-ItemProperty | Where-Object { $_.DisplayName -eq 'Harmony Work' -and $_.InstallLocation })
 if($keys.Count -ne 1){throw 'Existing per-user Harmony Work installation not found. Use the full offline installer.'}
 $key=$keys[0]; $actual=[IO.Path]::GetFullPath($key.InstallLocation.Trim('"')).TrimEnd('\')
 if($InstallDirectory -and [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\') -ne $actual){throw 'Requested directory does not match the registered installation'}
 $InstallDirectory=$actual
 if(!(Test-Path -LiteralPath (Join-Path $actual 'uninstall.exe'))){throw 'Existing uninstaller missing; repair with full installer'}
 $originalExe=SafeJoin $actual 'harmony-work.exe'
 if(!(Test-Path -LiteralPath $originalExe -PathType Leaf)){throw 'Existing Harmony Work executable missing; use the full offline installer'}
 # Only relaunch the registered, validated product; never an arbitrary /D path or fresh payload.
 $canRestart=((Get-Item -LiteralPath $originalExe).VersionInfo.ProductName -eq 'Harmony Work')
 $manifest=Get-Content -Raw -LiteralPath (Join-Path $Payload 'payload.manifest.json') | ConvertFrom-Json
 if($manifest.schemaVersion -ne 1 -or $manifest.mainExe -ne 'harmony-work.exe'){throw 'Invalid manifest schema'}
 foreach($f in $manifest.files) { if(!(OwnedPath $f.path)){throw 'Forbidden overlay destination'}; $null=SafeJoin $actual $f.path }
 if($Automatic -and !$Check){WaitForProductExit $originalExe}
 Log 'Verifying update payload and installed dependencies.'
 Verify $Payload $manifest.files
 Verify $actual $manifest.dependencies
 $runtimeRoot=Join-Path $env:LOCALAPPDATA 'Harmony\WorkBuddy\runtime'
 VerifyManagedRuntime $runtimeRoot $manifest
 SmokeRuntime $runtimeRoot
 $webview=@('HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKLM:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKCU:\Software\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}','HKLM:\Software\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}') | Where-Object { (Get-ItemProperty $_ -ErrorAction SilentlyContinue).pv -match '^[1-9]' }
 if(!$webview){throw 'WebView2 runtime missing. Use the full offline installer.'}
 $required=100MB + 3 * (($manifest.files | Measure-Object -Property size -Sum).Sum)
 $drive=New-Object IO.DriveInfo([IO.Path]::GetPathRoot($actual))
 if($drive.AvailableFreeSpace -lt $required){throw 'Insufficient disk space for staging and rollback backups'}
 ProbeInstallWrite $actual
 if($Check){exit 0}
 $exe=Join-Path $actual $manifest.mainExe
 $old=@(GetRunningProduct $exe)
 if($old -and !$Automatic){throw 'Please choose Exit from the Harmony Work tray menu, then run the lightweight installer again.'}
 WaitForProductExit $exe
 foreach($f in $manifest.files|Where-Object {$_.path.EndsWith('.exe')}){WaitForInstallAccess (SafeJoin $actual $f.path)}
 Log 'Preparing recovery and staged application files.'
 try { RecoverTransactions $actual $key } catch { $recoveryFailed=$true;throw }
 $transaction=Join-Path $actual ('.harmony-update-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path "$transaction\stage","$transaction\backup" | Out-Null
 foreach($f in $manifest.files){$stage=SafeJoin "$transaction\stage" $f.path; New-Item -ItemType Directory -Force (Split-Path -Parent $stage)|Out-Null; Copy-Item -LiteralPath (SafeJoin $Payload $f.path) -Destination $stage}
 Verify "$transaction\stage" $manifest.files
 WriteJournal 'pending'
 foreach($f in $manifest.files){Log "Installing $($f.path)";$target=SafeJoin $actual $f.path;$backup=SafeJoin "$transaction\backup" $f.path;$existed=Test-Path -LiteralPath $target; if($existed){New-Item -ItemType Directory -Force (Split-Path -Parent $backup)|Out-Null;Copy-Item -LiteralPath $target -Destination $backup};$backupInfo=if($existed){Get-Item -LiteralPath $backup}else{$null};$backupHash=if($existed){(Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash}else{$null};$changed+=@{path=$f.path;target=$target;backup=$backup;existed=$existed;size=$backupInfo.Length;sha256=$backupHash};WriteJournal 'pending';New-Item -ItemType Directory -Force (Split-Path -Parent $target)|Out-Null;Copy-Item -LiteralPath (SafeJoin "$transaction\stage" $f.path) -Destination $target -Force}
 $health=Join-Path $transaction 'health.json';$token=[guid]::NewGuid().ToString('N')
 $env:HARMONY_UPDATE_HEALTH_FILE=$health;$env:HARMONY_UPDATE_HEALTH_TOKEN=$token
 Log 'Starting the updated client and waiting for startup acknowledgement.'
 $child=Start-Process -FilePath $exe -PassThru
 Remove-Item Env:HARMONY_UPDATE_HEALTH_FILE,Env:HARMONY_UPDATE_HEALTH_TOKEN
 $deadline=(Get-Date).AddSeconds(60);$healthy=$false
 do {
  Start-Sleep -Seconds 1;$child.Refresh()
  if($child.HasExited){throw 'Updated application exited during startup'}
  if(Test-Path -LiteralPath $health) {
   $ack=$null
   try {$ack=Get-Content -Raw -LiteralPath $health|ConvertFrom-Json} catch {continue} # A partially published ack may become complete next poll.
   if($ack.token -ne $token -or $ack.version -ne $manifest.version){throw 'Updated application health acknowledgement does not match this transaction'}
   $healthy=$true;break
  }
 }while((Get-Date) -lt $deadline)
 if(!$healthy){throw 'Updated application did not acknowledge healthy startup'}
 Start-Sleep -Seconds 15;$child.Refresh();if($child.HasExited){throw 'Updated application failed stability check'}
 Set-ItemProperty -LiteralPath $key.PSPath -Name DisplayVersion -Value $manifest.version
 WriteJournal 'committed'
 Log "SUCCESS version=$($manifest.version) directory=$actual"
 Remove-Item -LiteralPath $transaction -Recurse -Force -ErrorAction SilentlyContinue
 exit 0
} catch {
 $failure=$_.Exception.Message
 if($child -and !$child.HasExited){Stop-Process -Id $child.Id -Force -ErrorAction SilentlyContinue; $child.WaitForExit(10000)|Out-Null}
 $rollbackErrors=@()
 [array]::Reverse($changed)
 foreach($c in $changed){try{if($c.existed){Copy-Item -LiteralPath $c.backup -Destination $c.target -Force}else{Remove-Item -LiteralPath $c.target -Force -ErrorAction Stop}}catch{$rollbackErrors+=$_.Exception.Message}}
 Log "FAILED $failure rollbackErrors=$($rollbackErrors -join '; ') backup=$transaction"
 if(!$Check -and $canRestart -and !$rollbackErrors.Count -and !$recoveryFailed) {
  $running=@(GetRunningProduct $originalExe)
  if(!$running){Start-Process -FilePath $originalExe -ErrorAction SilentlyContinue}
 }
 if(!$Silent){Add-Type -AssemblyName PresentationFramework;[Windows.MessageBox]::Show("Harmony Work update failed: $failure`nLog: $log`nRollback errors: $($rollbackErrors -join '; ')",'Harmony Work update')|Out-Null}
 exit 1
} finally {if($mutex){try{$mutex.ReleaseMutex()}catch{};$mutex.Dispose()}}
