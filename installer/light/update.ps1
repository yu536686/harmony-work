param([Parameter(Mandatory=$true)][string]$Payload,[string]$InstallDirectory,[switch]$Silent,[switch]$Check,[switch]$Automatic,[string]$ProgressFile)
$ErrorActionPreference='Stop'
$logRoot=if($Check){Join-Path $env:TEMP 'HarmonyWorkUpdateCheck'}else{Join-Path $env:LOCALAPPDATA 'Harmony\WorkBuddy\updates'}
New-Item -ItemType Directory -Force $logRoot | Out-Null
$log=Join-Path $logRoot 'light-installer.log'
. (Join-Path $PSScriptRoot 'progress.ps1')
function Progress($phase,$detail,$completed=$null,$total=$null,[bool]$safeRestart=$false,[string]$restartPath='') {try {WriteUpdateProgress -Path $ProgressFile -Phase $phase -Detail $detail -Completed $completed -Total $total -SafeRestart $safeRestart -RestartPath $restartPath}catch{}}
function Log($text) { Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) $text"; Write-Host $text }
function SafeJoin($root,$relative) {
 if($relative -notmatch '^[a-zA-Z0-9_. /@+\[\]()-]+$' -or $relative.StartsWith('/') -or @($relative.Split('/') | Where-Object { !$_ -or $_ -eq '.' -or $_ -eq '..' -or $_.EndsWith('.') -or $_.EndsWith(' ') }).Count) { throw "Unsafe manifest path: $relative" }
 $p=Join-Path $root ($relative.Replace('/','\'))
 $cursor=$p
 while($cursor -and $cursor.Length -ge $root.Length) { if((Test-Path -LiteralPath $cursor) -and ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Reparse point forbidden: $cursor" }; $cursor=Split-Path -Parent $cursor }
 return $p
}
function Verify($root,$items,[string]$description='') {
 $done=0;$count=@($items).Count
 foreach($f in $items) {
  $p=SafeJoin $root $f.path
  if(!(Test-Path -LiteralPath $p -PathType Leaf) -or (Get-Item -LiteralPath $p).Length -ne $f.size -or (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash -ne $f.sha256){throw "Missing or incompatible dependency/payload: $($f.path). Use the full offline installer."}
  $done++
  if($description -and ($done % 25 -eq 0 -or $done -eq $count)){Progress 'verifying' $description $done $count}
 }
}
function VerifyManagedRuntime($root,$manifest) {
 if(!$manifest.managedRuntimeProfiles){Verify $root $manifest.managedDependencies '正在校验运行环境';return}
 $id=if((Test-Path -LiteralPath (Join-Path $root 'python/python312._pth')) -and (Test-Path -LiteralPath (Join-Path $root 'python/python312.zip'))){'python-embedded'}else{'python-standard'}
 $profiles=@($manifest.managedRuntimeProfiles|Where-Object {$_.id -eq $id})
 if($profiles.Count -ne 1 -or !@($profiles[0].files).Count){throw "Unsupported managed runtime layout: $id. Use the full offline installer."}
 Verify $root $profiles[0].files '正在校验运行环境'
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
  if(!$announced){Progress 'waiting' '正在等待客户端退出，请暂时不要重新打开';Log 'Waiting for Harmony Work to exit. Do not reopen the client until the update completes.';$announced=$true}
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
function AssertSettingsPath([string]$path) {
 if(!$path -or $path -notmatch '^[a-zA-Z]:\\' -or $path.IndexOf(':',2) -ge 0 -or $path -match '[\x00-\x1f]' -or $path -match '(^|\\)(\.|\.\.)(\\|$)' -or $path -match '[. ](\\|$)'){throw 'Unsafe settings path'}
 $full=[IO.Path]::GetFullPath($path)
 $cursor=$full
 while($cursor) {
  try {if([IO.File]::GetAttributes($cursor) -band [IO.FileAttributes]::ReparsePoint){throw "Reparse point forbidden: $cursor"}}
  catch [IO.FileNotFoundException] {} catch [IO.DirectoryNotFoundException] {}
  $cursor=Split-Path -Parent $cursor
 }
}
function GetSettingsPath {
 $profile=[Environment]::GetFolderPath('UserProfile').TrimEnd('\')
 AssertSettingsPath $profile
 AssertSettingsPath $env:APPDATA
 $appData=[IO.Path]::GetFullPath($env:APPDATA).TrimEnd('\')
 $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
 if($sid -in @('S-1-5-18','S-1-5-19','S-1-5-20') -or (Split-Path -Leaf $profile) -in @('Default','Default User','Public','systemprofile','LocalService','NetworkService') -or !$appData.StartsWith($profile+'\',[StringComparison]::OrdinalIgnoreCase) -or !(Test-Path -LiteralPath $appData -PathType Container)){throw 'Settings must belong to the current non-default user profile'}
 $config=Join-Path $env:APPDATA 'com.harmony.xiaoheng-workbuddy\config.json'
 AssertSettingsPath $config
 return $config
}
function SetSettingsPrivateAcl([string]$path) {
 AssertSettingsPath $path
 $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
 $isDirectory=(Get-Item -LiteralPath $path -Force).PSIsContainer
 $acl=if($isDirectory){New-Object Security.AccessControl.DirectorySecurity}else{New-Object Security.AccessControl.FileSecurity}
 $acl.SetOwner($sid);$acl.SetAccessRuleProtection($true,$false)
 $inherit=if($isDirectory){[Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'}else{[Security.AccessControl.InheritanceFlags]::None}
 $rule=New-Object Security.AccessControl.FileSystemAccessRule($sid,[Security.AccessControl.FileSystemRights]::FullControl,$inherit,[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow)
 $acl.AddAccessRule($rule)
 Set-Acl -LiteralPath $path -AclObject $acl
}
function ReadSettingsBytes([string]$path,[int]$maximum) {
 AssertSettingsPath $path
 $stream=$null
 try {
  $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
  if($stream.Length -gt $maximum){throw 'Settings snapshot exceeds size limit'}
  $bytes=New-Object byte[] ([int]$stream.Length)
  $offset=0
  while($offset -lt $bytes.Length){$read=$stream.Read($bytes,$offset,$bytes.Length-$offset);if(!$read){throw 'Incomplete settings snapshot read'};$offset+=$read}
  return ,$bytes
 } finally {if($stream){$stream.Dispose()}}
}
function SettingsHash([byte[]]$bytes) {
 $hash=[Security.Cryptography.SHA256]::Create()
 try{return [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','')}finally{$hash.Dispose()}
}
function SettingsEntropy([string]$directory) {
 # Bind the protected envelope to this transaction, not a journal-supplied destination.
 return ,([Text.Encoding]::UTF8.GetBytes('HarmonyWork.LightSettings.v1|'+(Split-Path -Leaf $directory)))
}
function NewSettingsSnapshot([string]$directory) {
 Add-Type -AssemblyName System.Security
 AssertSettingsPath $directory
 $config=GetSettingsPath
 $bytes=$null;$envelope=$null
 try {
  $existed=Test-Path -LiteralPath $config
  if($existed){$bytes=ReadSettingsBytes $config 4MB}else{$bytes=New-Object byte[] 0}
  $envelope=New-Object byte[] ($bytes.Length+1)
  $envelope[0]=[byte][bool]$existed
  if($bytes.Length){[Array]::Copy($bytes,0,$envelope,1,$bytes.Length)}
  $protected=[Security.Cryptography.ProtectedData]::Protect($envelope,(SettingsEntropy $directory),[Security.Cryptography.DataProtectionScope]::CurrentUser)
  $blob=Join-Path $directory 'config.dpapi';AssertSettingsPath $blob
  $stream=$null
  try {
   $stream=[IO.File]::Open($blob,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
   $stream.Dispose();$stream=$null
   SetSettingsPrivateAcl $blob
   $stream=[IO.File]::Open($blob,[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::None)
   $stream.Write($protected,0,$protected.Length);$stream.Flush($true)
  } finally {if($stream){$stream.Dispose()}}
  return [pscustomobject]@{schemaVersion=1;snapshotTaken=$true;existed=[bool]$existed;protection='DPAPI-CurrentUser';blob='config.dpapi';sha256=(SettingsHash $protected);size=$protected.Length;configSize=$bytes.Length}
 } finally {
  if($null -ne $bytes){[Array]::Clear($bytes,0,$bytes.Length)}
  if($null -ne $envelope){[Array]::Clear($envelope,0,$envelope.Length)}
 }
}
function RestoreSettingsSnapshot([string]$directory,$metadata,[switch]$ValidateOnly) {
 Add-Type -AssemblyName System.Security
 $fields=@('schemaVersion','snapshotTaken','existed','protection','blob','sha256','size','configSize')
 if($null -eq $metadata -or @($metadata.PSObject.Properties).Count -ne $fields.Count -or @($metadata.PSObject.Properties.Name|Where-Object {$_ -cnotin $fields}).Count){throw 'Invalid settings snapshot metadata'}
 $validVersion=($metadata.schemaVersion -is [int] -or $metadata.schemaVersion -is [long]) -and ($metadata.schemaVersion -eq 1)
 $validSize=($metadata.size -is [int] -or $metadata.size -is [long]) -and ($metadata.size -gt 0) -and ($metadata.size -le (4MB+64KB))
 $validConfigSize=($metadata.configSize -is [int] -or $metadata.configSize -is [long]) -and ($metadata.configSize -ge 0) -and ($metadata.configSize -le 4MB)
 if(!$validVersion -or !$validSize -or !$validConfigSize -or $metadata.snapshotTaken -isnot [bool] -or !$metadata.snapshotTaken -or $metadata.existed -isnot [bool] -or $metadata.protection -isnot [string] -or $metadata.protection -cne 'DPAPI-CurrentUser' -or $metadata.blob -isnot [string] -or $metadata.blob -cne 'config.dpapi' -or $metadata.sha256 -isnot [string] -or $metadata.sha256 -cnotmatch '^[A-Fa-f0-9]{64}$' -or (!$metadata.existed -and $metadata.configSize -ne 0)){throw 'Invalid settings snapshot metadata'}
 $config=GetSettingsPath
 AssertSettingsPath $directory
 $protected=ReadSettingsBytes (Join-Path $directory 'config.dpapi') (4MB+64KB)
 if($protected.Length -ne $metadata.size -or (SettingsHash $protected) -ne $metadata.sha256){throw 'Settings snapshot hash or size mismatch'}
 $envelope=$null;$bytes=$null;$restoreDirectory=$null;$temp=$null
 try {
  $envelope=[Security.Cryptography.ProtectedData]::Unprotect($protected,(SettingsEntropy $directory),[Security.Cryptography.DataProtectionScope]::CurrentUser)
  if($envelope.Length -ne ($metadata.configSize+1) -or $envelope[0] -ne [byte]$metadata.existed){throw 'Invalid settings snapshot envelope'}
  if($ValidateOnly){return}
  if(!$metadata.existed){return}
  $bytes=New-Object byte[] $metadata.configSize
  if($bytes.Length){[Array]::Copy($envelope,1,$bytes,0,$bytes.Length)}
  $parent=Split-Path -Parent $config
  if(!(Test-Path -LiteralPath $parent)){New-Item -ItemType Directory -Path $parent|Out-Null;SetSettingsPrivateAcl $parent}
  # Plaintext exists only in a private same-volume restore directory, never in the transaction.
  $restoreDirectory=Join-Path $parent ('.harmony-config-restore-'+[guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Path $restoreDirectory|Out-Null;SetSettingsPrivateAcl $restoreDirectory
  $temp=Join-Path $restoreDirectory 'config.json'
  $stream=$null
  try {$stream=[IO.File]::Open($temp,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{if($stream){$stream.Dispose()}}
  SetSettingsPrivateAcl $temp
  AssertSettingsPath $config;AssertSettingsPath $temp
  # Windows PowerShell 5.1 marshals $null string arguments as empty paths, not CLR null.
  if(Test-Path -LiteralPath $config){[IO.File]::Replace($temp,$config,[NullString]::Value)}else{[IO.File]::Move($temp,$config)}
  SetSettingsPrivateAcl $config
 } finally {
  if($null -ne $envelope){[Array]::Clear($envelope,0,$envelope.Length)}
  if($null -ne $bytes){[Array]::Clear($bytes,0,$bytes.Length)}
  if($temp -and (Test-Path -LiteralPath $temp)){Remove-Item -LiteralPath $temp -Force}
  if($restoreDirectory -and (Test-Path -LiteralPath $restoreDirectory)){Remove-Item -LiteralPath $restoreDirectory -Force}
 }
}
function WriteJournal($state) {
 $journal=@{schemaVersion=2;installDirectory=$InstallDirectory;previousVersion=$key.DisplayVersion;state=$state;settingsSnapshot=$settingsSnapshot;changes=@($changed | ForEach-Object { @{path=$_.path;existed=$_.existed;sha256=$_.sha256;size=$_.size} })}
 $tmp=Join-Path $transaction 'journal.tmp'
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $tmp -Encoding UTF8
 Move-Item -LiteralPath $tmp -Destination (Join-Path $transaction 'journal.json') -Force
}
function RecoverTransactions($root,$registryKey) {
 foreach($dir in @(Get-ChildItem -LiteralPath $root -Directory -Filter '.harmony-update-*')) {
  $null=SafeJoin $root $dir.Name
  $journalPath=SafeJoin $dir.FullName 'journal.json'
  if(!(Test-Path -LiteralPath $journalPath)){continue}
  $j=Get-Content -Raw -LiteralPath $journalPath|ConvertFrom-Json
  $validSchema=($j.schemaVersion -is [int] -or $j.schemaVersion -is [long]) -and ($j.schemaVersion -in @(1,2))
  if(!$validSchema -or $j.installDirectory -isnot [string] -or $j.installDirectory -ne $root -or $j.state -isnot [string] -or $j.state -cnotin @('pending','committed','rolledBack') -or ($j.schemaVersion -eq 1 -and ($null -ne $j.settingsSnapshot -or (Test-Path -LiteralPath (Join-Path $dir.FullName 'config.dpapi'))))){throw "Invalid interrupted update journal: $journalPath"}
  if($j.schemaVersion -eq 2){RestoreSettingsSnapshot $dir.FullName $j.settingsSnapshot -ValidateOnly}
  if($j.state -eq 'pending') {
   $entries=@($j.changes);[array]::Reverse($entries)
   foreach($entry in $entries) {
    if(!(OwnedPath $entry.path)){throw 'Unsafe interrupted update journal destination'}
    $target=SafeJoin $root $entry.path
    if($entry.existed){Verify (Join-Path $dir.FullName 'backup') @($entry);Copy-Item -LiteralPath (SafeJoin (Join-Path $dir.FullName 'backup') $entry.path) -Destination $target -Force}
    elseif(Test-Path -LiteralPath $target){Remove-Item -LiteralPath $target -Force}
   }
   # Schema 1 predates settings migration; preserve recovery of those legacy app-only transactions.
   if($j.schemaVersion -eq 2){RestoreSettingsSnapshot $dir.FullName $j.settingsSnapshot}
   Set-ItemProperty -LiteralPath $registryKey.PSPath -Name DisplayVersion -Value $j.previousVersion
   Log "RECOVERED interrupted transaction $($dir.FullName)"
  }
  Remove-Item -LiteralPath $dir.FullName -Recurse -Force
 }
}
$changed=@(); $transaction=$null; $child=$null; $mutex=$null; $originalExe=$null; $canRestart=$false; $recoveryFailed=$false; $settingsSnapshot=$null
try {
 if(![Environment]::Is64BitProcess){throw 'The light updater requires 64-bit PowerShell. Run the signed lightweight installer instead of a 32-bit script host.'}
 $mutex=New-Object Threading.Mutex($false,'Local\HarmonyWorkLightUpdate'); if(!$mutex.WaitOne(0)){throw 'Another update is running'}
 Progress 'preparing' '正在准备更新'
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
 Progress 'verifying' '正在校验更新内容'
 Log 'Verifying update payload and installed dependencies.'
 Verify $Payload $manifest.files '正在校验更新内容'
 Verify $actual $manifest.dependencies '正在校验应用组件'
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
 Progress 'staging' '正在备份当前版本'
 Log 'Preparing recovery and staged application files.'
 try { RecoverTransactions $actual $key } catch { $recoveryFailed=$true;throw }
 $transaction=Join-Path $actual ('.harmony-update-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path $transaction | Out-Null
 SetSettingsPrivateAcl $transaction
 New-Item -ItemType Directory -Path "$transaction\stage","$transaction\backup" | Out-Null
 foreach($f in $manifest.files){$stage=SafeJoin "$transaction\stage" $f.path; New-Item -ItemType Directory -Force (Split-Path -Parent $stage)|Out-Null; Copy-Item -LiteralPath (SafeJoin $Payload $f.path) -Destination $stage}
 Verify "$transaction\stage" $manifest.files
 try {$settingsSnapshot=NewSettingsSnapshot $transaction} catch {$recoveryFailed=$true;throw}
 WriteJournal 'pending'
 $copied=0
 foreach($f in $manifest.files){Log "Installing $($f.path)";$target=SafeJoin $actual $f.path;$backup=SafeJoin "$transaction\backup" $f.path;$existed=Test-Path -LiteralPath $target; if($existed){New-Item -ItemType Directory -Force (Split-Path -Parent $backup)|Out-Null;Copy-Item -LiteralPath $target -Destination $backup};$backupInfo=if($existed){Get-Item -LiteralPath $backup}else{$null};$backupHash=if($existed){(Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash}else{$null};$changed+=@{path=$f.path;target=$target;backup=$backup;existed=$existed;size=$backupInfo.Length;sha256=$backupHash};WriteJournal 'pending';New-Item -ItemType Directory -Force (Split-Path -Parent $target)|Out-Null;Copy-Item -LiteralPath (SafeJoin "$transaction\stage" $f.path) -Destination $target -Force;$copied++;Progress 'copying' '正在安装更新' $copied @($manifest.files).Count}
 $health=Join-Path $transaction 'health.json';$token=[guid]::NewGuid().ToString('N')
 $env:HARMONY_UPDATE_HEALTH_FILE=$health;$env:HARMONY_UPDATE_HEALTH_TOKEN=$token
 Progress 'starting' '正在启动新版客户端'
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
 Progress 'starting' '正在确认新版运行正常'
 Start-Sleep -Seconds 15;$child.Refresh();if($child.HasExited){throw 'Updated application failed stability check'}
 Set-ItemProperty -LiteralPath $key.PSPath -Name DisplayVersion -Value $manifest.version
 WriteJournal 'committed'
 Log "SUCCESS version=$($manifest.version) directory=$actual"
 Remove-Item -LiteralPath $transaction -Recurse -Force -ErrorAction SilentlyContinue
 Progress 'complete' '更新已完成，正在返回客户端'
 exit 0
} catch {
 $failure=$_.Exception.Message
 Remove-Item Env:HARMONY_UPDATE_HEALTH_FILE,Env:HARMONY_UPDATE_HEALTH_TOKEN -ErrorAction SilentlyContinue
 Progress 'rollback' '正在恢复原版本'
 $rollbackErrors=@()
 if($transaction) {
  try {
   if($child -and !$child.HasExited){Stop-Process -Id $child.Id -Force; if(!$child.WaitForExit(10000)){throw 'Updated application did not exit for rollback'}}
   WaitForProductExit $originalExe 10
   AssertSettingsPath $transaction
   if($settingsSnapshot){RestoreSettingsSnapshot $transaction $settingsSnapshot -ValidateOnly}
  } catch {$rollbackErrors+=$_.Exception.Message}
  if(!$rollbackErrors.Count) {
   [array]::Reverse($changed)
   foreach($c in $changed){try{$target=SafeJoin $InstallDirectory $c.path;if($c.existed){Verify (Join-Path $transaction 'backup') @($c);Copy-Item -LiteralPath $c.backup -Destination $target -Force}elseif(Test-Path -LiteralPath $target){Remove-Item -LiteralPath $target -Force -ErrorAction Stop}}catch{$rollbackErrors+=$_.Exception.Message}}
   if($settingsSnapshot){try{RestoreSettingsSnapshot $transaction $settingsSnapshot}catch{$rollbackErrors+=$_.Exception.Message}}
   if(!$rollbackErrors.Count -and $changed.Count){try{Set-ItemProperty -LiteralPath $key.PSPath -Name DisplayVersion -Value $key.DisplayVersion}catch{$rollbackErrors+=$_.Exception.Message}}
   # Do not replay an already restored snapshot over edits made after the old client restarts.
   if(!$rollbackErrors.Count -and $settingsSnapshot){try{WriteJournal 'rolledBack'}catch{$rollbackErrors+=$_.Exception.Message}}
  }
 }
 Log "FAILED $failure rollbackErrors=$($rollbackErrors -join '; ') backup=$transaction"
 $safeRestart=(!$Check -and $canRestart -and !$rollbackErrors.Count -and !$recoveryFailed)
 $message=if($failure -like '*still running*' -or $failure -like 'Please choose Exit*'){'请先从托盘退出客户端，再点击重试。'}elseif($failure -like 'Missing or incompatible*' -or $failure -like '*Runtime preflight*'){'运行环境校验未通过，请查看日志或使用完整安装包。'}elseif($safeRestart){'更新未完成，原版本已恢复。可以重试或返回客户端。'}else{'更新未完成。请查看日志或使用完整安装包修复。'}
 Progress 'failed' $message $null $null $safeRestart $originalExe
 if($safeRestart -and !$ProgressFile) {
  $running=@(GetRunningProduct $originalExe)
  if(!$running){Start-Process -FilePath $originalExe -ErrorAction SilentlyContinue}
 }
 if(!$Silent -and !$ProgressFile){Add-Type -AssemblyName PresentationFramework;[Windows.MessageBox]::Show("Harmony Work update failed: $failure`nLog: $log`nRollback errors: $($rollbackErrors -join '; ')",'Harmony Work update')|Out-Null}
 exit 1
} finally {if($mutex){try{$mutex.ReleaseMutex()}catch{};$mutex.Dispose()}}
