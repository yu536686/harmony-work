$ErrorActionPreference='Stop'
$module=Join-Path (Split-Path -Parent $PSScriptRoot) 'installer\light\progress.ps1'
if(!(Test-Path $module)){throw 'Progress protocol is not implemented'}
. $module
$root=Join-Path $env:TEMP ('HarmonyProgressTest-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $root|Out-Null
try {
 $file=Join-Path $root 'progress.json'
 WriteUpdateProgress -Path $file -Phase verifying -Detail '校验运行环境' -Completed 25 -Total 100
 $state=ReadUpdateProgress $file
 if($state.phase -ne 'verifying' -or $state.completed -ne 25 -or $state.total -ne 100 -or $state.terminal){throw 'Measurable progress lost counts'}
 Write-Output 'PASS file verification publishes real stage counts'
 WriteUpdateProgress -Path $file -Phase waiting -Detail '等待客户端退出'
 $state=ReadUpdateProgress $file
 if($null -ne $state.total -or $state.terminal){throw 'Unknown duration must be indeterminate'}
 Write-Output 'PASS unknown durations never invent a percentage'
 foreach($counts in @(@(-1,10),@(11,10),@(1,0))){
  $rejected=$false
  try {WriteUpdateProgress -Path $file -Phase copying -Detail test -Completed $counts[0] -Total $counts[1]}catch{$rejected=$true}
  if(!$rejected){throw 'Invalid progress counts accepted'}
 }
 Write-Output 'PASS invalid progress counts are rejected'
 WriteUpdateProgress -Path $file -Phase failed -Detail '更新未完成' -SafeRestart $false
 if((ReadUpdateProgress $file).safeRestart -or !(ReadUpdateProgress $file).terminal){throw 'Unsafe rollback must not restart'}
 WriteUpdateProgress -Path $file -Phase failed -Detail '已恢复原版本' -SafeRestart $true -RestartPath 'C:\fixture\harmony-work.exe'
 if(!(ReadUpdateProgress $file).safeRestart){throw 'Safe rollback result lost'}
 Write-Output 'PASS failure explicitly controls safe old-client restart'
 for($i=0;$i -le 100;$i++){
  WriteUpdateProgress -Path $file -Phase copying -Detail '安装更新' -Completed $i -Total 100
  if((ReadUpdateProgress $file).completed -ne $i){throw 'Atomic progress replacement lost a state'}
 }
 if(@(Get-ChildItem $root -Filter '*.tmp').Count){throw 'Temporary progress files leaked'}
 Write-Output 'PASS repeated atomic replacement is readable without stale temp files'
 [IO.File]::WriteAllText($file,'{"phase":')
 if($null -ne (ReadUpdateProgress $file)){throw 'Partial JSON should be ignored'}
 Write-Output 'PASS incomplete telemetry is ignored without crashing the UI'
}finally{Remove-Item $root -Recurse -Force}
