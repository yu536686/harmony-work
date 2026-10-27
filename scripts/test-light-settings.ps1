param([string]$TestRoot)
$ErrorActionPreference='Stop'
if([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT){throw 'Windows DPAPI behavior test: run on Windows, not a source-contract substitute'}
$installer=Join-Path (Split-Path -Parent $PSScriptRoot) 'installer\light\update.ps1'
$source=Get-Content -LiteralPath $installer -Raw
$from=$source.IndexOf('function Progress(');$to=$source.IndexOf('$changed=@()')
if($from -lt 0 -or $to -le $from){throw 'Installer function boundary not found'}
. ([ScriptBlock]::Create($source.Substring($from,$to-$from)))
function Assert($condition,$message){if(!$condition){throw $message}}
function Refuses([scriptblock]$action,$message){$refused=$false;try{& $action}catch{$refused=$true};Assert $refused $message}
function SameBytes($path,[byte[]]$expected){Assert ([Convert]::ToBase64String([IO.File]::ReadAllBytes($path)) -ceq [Convert]::ToBase64String($expected)) "Bytes changed: $path"}
function CloneMetadata($metadata){return ($metadata|ConvertTo-Json|ConvertFrom-Json)}
if(!$TestRoot){$TestRoot=Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'HarmonyUpdateSettingsTests'}
$fixture=Join-Path $TestRoot ([guid]::NewGuid().ToString('N'))
AssertSettingsPath $fixture
New-Item -ItemType Directory -Path $fixture -Force|Out-Null
SetSettingsPrivateAcl $fixture
$savedAppData=$env:APPDATA;$registryPath=$null
$log=Join-Path $fixture 'test.log'
try {
 $env:APPDATA=Join-Path $fixture 'synthetic-appdata'
 New-Item -ItemType Directory $env:APPDATA|Out-Null
 $config=GetSettingsPath
 New-Item -ItemType Directory (Split-Path -Parent $config)|Out-Null
 $database=Join-Path (Split-Path -Parent $config) 'workbuddy.db';[IO.File]::WriteAllText($database,'database-sentinel')
 $workspace=Join-Path $fixture 'workspace';New-Item -ItemType Directory $workspace|Out-Null
 $workspaceFile=Join-Path $workspace 'document.txt';[IO.File]::WriteAllText($workspaceFile,'workspace-sentinel')
 $install=Join-Path $fixture 'install';New-Item -ItemType Directory $install|Out-Null
 $transaction=Join-Path $install ('.harmony-update-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory $transaction|Out-Null;SetSettingsPrivateAcl $transaction
 [byte[]]$original=@(0xEF,0xBB,0xBF)+[Text.Encoding]::UTF8.GetBytes("{`r`n  `"vision_api_key`": `"synthetic-plaintext-key-113`", `"value`": 17`r`n}`r`n")
 [byte[]]$migrated=[Text.Encoding]::UTF8.GetBytes('{"vision_api_key":"new-encrypted-format"}')
 [IO.File]::WriteAllBytes($config,$original)
 $metadata=NewSettingsSnapshot $transaction
 Assert $metadata.snapshotTaken 'Snapshot flag missing'
 Assert ($metadata.protection -ceq 'DPAPI-CurrentUser') 'Wrong protection scope'
 foreach($file in Get-ChildItem -LiteralPath $transaction -File -Recurse){Assert (![Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($file.FullName)).Contains('synthetic-plaintext-key-113')) 'plaintext backup found'}
 Assert (@(Get-ChildItem -LiteralPath $transaction -File).Count -eq 1) 'Unexpected snapshot files'
 $acl=Get-Acl -LiteralPath (Join-Path $transaction 'config.dpapi')
 $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
 Assert $acl.AreAccessRulesProtected 'Snapshot file ACL is not deterministic/private'
 foreach($rule in $acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier])){Assert ($rule.IdentityReference -eq $sid -and $rule.AccessControlType -eq 'Allow') 'Non-user snapshot access'}
 [IO.File]::WriteAllBytes($config,$migrated)
 RestoreSettingsSnapshot $transaction $metadata
 SameBytes $config $original
 Assert (@(Get-ChildItem -LiteralPath (Split-Path -Parent $config) -Directory -Filter '.harmony-config-restore-*').Count -eq 0) 'Atomic overwrite left a restore directory'
 Assert (@(Get-ChildItem -LiteralPath (Split-Path -Parent $config) -File|Where-Object {$_.Name -notin @('config.json','workbuddy.db')}).Count -eq 0) 'Atomic overwrite left a backup file'
 Write-Output 'PASS native PowerShell atomic overwrite of existing config uses no backup file'
 Remove-Item -LiteralPath $config
 RestoreSettingsSnapshot $transaction $metadata
 SameBytes $config $original
 Write-Output 'PASS actual CurrentUser DPAPI snapshot restores original bytes; no plaintext backup; user-only ACL'

 [IO.File]::WriteAllBytes($config,$migrated)
 $bad=CloneMetadata $metadata;$bad.sha256='0'*64
 Refuses {RestoreSettingsSnapshot $transaction $bad} 'wrong hash was accepted'
 SameBytes $config $migrated
 Assert (Test-Path -LiteralPath (Join-Path $transaction 'config.dpapi')) 'Failure deleted evidence'
 foreach($field in @('schemaVersion','snapshotTaken','existed','blob','size','configSize','protection')) {
  $bad=CloneMetadata $metadata
  switch($field){'schemaVersion'{$bad.schemaVersion='1'};'snapshotTaken'{$bad.snapshotTaken=$false};'existed'{$bad.existed='true'};'blob'{$bad.blob='../config.json'};'size'{$bad.size=-1};'configSize'{$bad.configSize=4MB+1};'protection'{$bad.protection='DPAPI-LocalMachine'}}
  Refuses {RestoreSettingsSnapshot $transaction $bad} "Invalid metadata accepted: $field"
  SameBytes $config $migrated
 }
 $bad=CloneMetadata $metadata;$bad.existed=$false;$bad.configSize=0
 Refuses {RestoreSettingsSnapshot $transaction $bad} 'Tampered absent flag accepted'
 $blob=Join-Path $transaction 'config.dpapi';$protected=[IO.File]::ReadAllBytes($blob)
 $tampered=$protected.Clone();$tampered[$tampered.Length-1]=$tampered[$tampered.Length-1] -bxor 1
 [IO.File]::WriteAllBytes($blob,$tampered)
 $bad=CloneMetadata $metadata;$bad.sha256=SettingsHash $tampered
 Refuses {RestoreSettingsSnapshot $transaction $bad} 'DPAPI accepted tampered blob with recomputed hash'
 SameBytes $config $migrated
 [IO.File]::WriteAllBytes($blob,$protected)
 $otherTransaction=Join-Path $fixture 'different-transaction';New-Item -ItemType Directory $otherTransaction|Out-Null
 [IO.File]::WriteAllBytes((Join-Path $otherTransaction 'config.dpapi'),$protected)
 Refuses {RestoreSettingsSnapshot $otherTransaction $metadata} 'Snapshot accepted outside its original transaction'
 SameBytes $config $migrated
 Write-Output 'PASS wrong hash, invalid metadata and tampered DPAPI blob refused without config mutation or evidence deletion'

 Remove-Item -LiteralPath $config
 $absentTransaction=Join-Path $fixture 'absent';New-Item -ItemType Directory $absentTransaction|Out-Null;SetSettingsPrivateAcl $absentTransaction
 $absent=NewSettingsSnapshot $absentTransaction
 RestoreSettingsSnapshot $absentTransaction $absent
 Assert (!(Test-Path -LiteralPath $config)) 'absent config unexpectedly created'
 [IO.File]::WriteAllBytes($config,$migrated)
 RestoreSettingsSnapshot $absentTransaction $absent
 SameBytes $config $migrated
 Write-Output 'PASS absent prior config does not delete unrelated/new config'
 Remove-Item -LiteralPath $config
 [IO.File]::WriteAllBytes($config,(New-Object byte[] 0))
 $emptyTransaction=Join-Path $fixture 'empty';New-Item -ItemType Directory $emptyTransaction|Out-Null
 $empty=NewSettingsSnapshot $emptyTransaction
 [IO.File]::WriteAllBytes($config,$migrated)
 RestoreSettingsSnapshot $emptyTransaction $empty
 SameBytes $config (New-Object byte[] 0)
 Assert $empty.existed 'Empty config mistaken for absent config'
 Write-Output 'PASS empty config is restored distinctly from absent config'

 $large=Join-Path $fixture 'oversize';New-Item -ItemType Directory $large|Out-Null
 [IO.File]::WriteAllBytes($config,(New-Object byte[] (4MB+1)))
 Refuses {NewSettingsSnapshot $large} 'Oversized config accepted'
 Assert (!(Test-Path -LiteralPath (Join-Path $large 'config.dpapi'))) 'Oversized config left snapshot'
 [IO.File]::WriteAllBytes($config,$migrated)
 $realAppData=$env:APPDATA
 try {
  $env:APPDATA='C:\Users\Default\AppData\Roaming';Refuses {GetSettingsPath} 'Default profile accepted'
  $env:APPDATA='relative-appdata';Refuses {GetSettingsPath} 'Relative APPDATA accepted'
  $env:APPDATA='\\localhost\share';Refuses {GetSettingsPath} 'UNC APPDATA accepted'
 }finally{$env:APPDATA=$realAppData}
 $junction=Join-Path $fixture 'linked-appdata'
 New-Item -ItemType Junction -Path $junction -Target $realAppData|Out-Null
 try{$env:APPDATA=$junction;Refuses {GetSettingsPath} 'Reparse APPDATA accepted'}finally{$env:APPDATA=$realAppData;[IO.Directory]::Delete($junction)}
 Write-Output 'PASS size bound, default/relative/UNC paths and reparse paths refused'

 # Exercise the actual interrupted recovery function using only a synthetic install and HKCU key.
 $registryPath='HKCU:\Software\HarmonyLightSettingsTest-'+[guid]::NewGuid().ToString('N')
 New-Item -Path $registryPath|Out-Null
 $registryKey=Get-Item $registryPath
 $target=Join-Path $install 'harmony-work.exe';[IO.File]::WriteAllText($target,'new-app')
 $backup=Join-Path $transaction 'backup';New-Item -ItemType Directory $backup|Out-Null
 [IO.File]::WriteAllText((Join-Path $backup 'harmony-work.exe'),'old-app')
 $journal=@{schemaVersion=2;installDirectory=$install;previousVersion='1.1.2';state='pending';settingsSnapshot=$metadata;changes=@(@{path='harmony-work.exe';existed=$true;size=7;sha256=(Get-FileHash -LiteralPath (Join-Path $backup 'harmony-work.exe')).Hash})}
 $journalPath=Join-Path $transaction 'journal.json'
 $bad=CloneMetadata $metadata;$bad.sha256='0'*64;$journal.settingsSnapshot=$bad
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journalPath -Encoding UTF8
 Refuses {RecoverTransactions $install $registryKey} 'Recovery accepted corrupt snapshot'
 Assert (Test-Path -LiteralPath $journalPath) 'Failed recovery removed journal'
 Assert ([IO.File]::ReadAllText($target) -ceq 'new-app') 'Invalid recovery mutated app before validation'
 SameBytes $config $migrated
 $journal.settingsSnapshot=$null
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journalPath -Encoding UTF8
 Refuses {RecoverTransactions $install $registryKey} 'Recovery accepted missing snapshot metadata'
 Assert (Test-Path -LiteralPath $journalPath) 'Missing metadata deleted evidence'
 # A completed rollback/commit must only clean up; never overwrite later user edits.
 $journal.settingsSnapshot=$metadata
 foreach($state in @('rolledBack','committed')) {
  $journal.state=$state
  $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journalPath -Encoding UTF8
  RecoverTransactions $install $registryKey
  SameBytes $config $migrated
  Assert (!(Test-Path -LiteralPath $transaction)) 'Completed transaction was not cleaned'
  New-Item -ItemType Directory $transaction|Out-Null;SetSettingsPrivateAcl $transaction
  [IO.File]::WriteAllBytes((Join-Path $transaction 'config.dpapi'),$protected)
  SetSettingsPrivateAcl (Join-Path $transaction 'config.dpapi')
  New-Item -ItemType Directory $backup|Out-Null
  [IO.File]::WriteAllText((Join-Path $backup 'harmony-work.exe'),'old-app')
 }
 $journal.state='pending'
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journalPath -Encoding UTF8
 RecoverTransactions $install $registryKey
 SameBytes $config $original
 Assert ([IO.File]::ReadAllText($target) -ceq 'old-app') 'Old app not recovered'
 Assert (!(Test-Path -LiteralPath $transaction)) 'Recovered snapshot not cleaned'
 Assert ((Get-ItemProperty -LiteralPath $registryPath).DisplayVersion -ceq '1.1.2') 'Registry version not recovered'
 Write-Output 'PASS interrupted transaction restores both app and settings; invalid recovery preserves evidence'
 New-Item -ItemType Directory $transaction|Out-Null
 New-Item -ItemType Directory $backup|Out-Null
 [IO.File]::WriteAllText((Join-Path $backup 'harmony-work.exe'),'old-app')
 $journal.Remove('settingsSnapshot');$journal.schemaVersion=1
 $journal|ConvertTo-Json -Depth 6|Set-Content -LiteralPath $journalPath -Encoding UTF8
 [IO.File]::WriteAllBytes($config,$migrated)
 RecoverTransactions $install $registryKey
 SameBytes $config $migrated
 Assert ([IO.File]::ReadAllText($database) -ceq 'database-sentinel') 'Database changed'
 Assert ([IO.File]::ReadAllText($workspaceFile) -ceq 'workspace-sentinel') 'Workspace changed'
 Write-Output 'PASS legacy app-only recovery remains supported; completed snapshots do not replay; DB/workspace untouched'
 Write-Output 'PASS Windows DPAPI behavior verified (no installed product or real APPDATA modified)'
} finally {
 $env:APPDATA=$savedAppData
 if($registryPath -and (Test-Path -LiteralPath $registryPath)){Remove-Item -LiteralPath $registryPath -Recurse -Force}
 Remove-Item -LiteralPath $fixture -Recurse -Force
}
