param([Parameter(Mandatory=$true)][string]$Payload,[string]$InstallDirectory,[switch]$Silent,[switch]$Check,[switch]$Automatic)
$ErrorActionPreference='Stop'
trap {
 $problem=$_.Exception.Message
 try {Add-Type -AssemblyName System.Windows.Forms;[Windows.Forms.MessageBox]::Show('无法启动更新窗口，更新尚未完成。请重新检查更新或使用完整安装包。'+[Environment]::NewLine+$problem,'Harmony Work')|Out-Null}catch{}
 exit 1
}
$worker=Join-Path $PSScriptRoot 'update.ps1'
if($Check){& $worker -Payload $Payload -InstallDirectory $InstallDirectory -Silent -Check;exit $LASTEXITCODE}
# This host only renders trusted local XAML and starts the signed package's fixed worker.
# The worker remains the sole owner of installation validation, transactions and rollback.
Add-Type -AssemblyName PresentationFramework,PresentationCore,WindowsBase
. (Join-Path $PSScriptRoot 'progress.ps1')
$progressFile=Join-Path $PSScriptRoot ('progress-'+[guid]::NewGuid().ToString('N')+'.json')
$script:workerProcess=$null;$script:lastState=$null;$script:allowClose=$false;$script:result=1;$script:displayState=''
. (Join-Path $PSScriptRoot 'window.ps1')
$window=NewBrandedUpdateWindow -Directory $PSScriptRoot
$names=@('BrandIcon','Heading','VersionLabel','StageText','PercentText','Progress','HintText','FailureActions','RetryButton','LogButton','CloseButton','MinimizeButton','DragRegion')
$controls=@{};foreach($name in $names){$controls[$name]=$window.FindName($name)}
$manifest=Get-Content -LiteralPath (Join-Path $Payload 'payload.manifest.json') -Raw -Encoding UTF8|ConvertFrom-Json
$controls.VersionLabel.Text='正在安装 v'+$manifest.version
$timer=New-Object Windows.Threading.DispatcherTimer;$timer.Interval=[TimeSpan]::FromMilliseconds(180)
$script:pulse=0
function StartWorker {
 if($script:workerProcess -and !$script:workerProcess.HasExited){return}
 if(Test-Path -LiteralPath $progressFile){Remove-Item -LiteralPath $progressFile -Force}
 $script:lastState=$null;$script:displayState='';$controls.Heading.Text='正在更新';$controls.StageText.Text='正在准备更新';$controls.PercentText.Text='';$controls.FailureActions.Visibility='Collapsed';$controls.HintText.Text='完成后将自动打开客户端，请暂时不要手动启动。'
 # EncodedCommand safely transports paths containing spaces without shell interpolation.
 $escape={param($v) "'"+$v.Replace("'","''")+"'"}
 $command='& '+(& $escape $worker)+' -Payload '+(& $escape $Payload)+' -InstallDirectory '+(& $escape $InstallDirectory)+' -Silent -ProgressFile '+(& $escape $progressFile)
 if($Automatic){$command+=' -Automatic'}
 $command+='; exit $LASTEXITCODE'
 $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
 $info=New-Object Diagnostics.ProcessStartInfo
 $info.FileName=Join-Path $PSHOME 'powershell.exe';$info.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand '+$encoded
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true
 $script:workerProcess=[Diagnostics.Process]::Start($info)
}
function SafeReturnToClient {
 $s=$script:lastState
 if(!$s -or !$s.safeRestart -or !$s.restartPath){return}
 try {
  $keys=@(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'|Get-ItemProperty|Where-Object {$_.DisplayName -eq 'Harmony Work' -and $_.InstallLocation})
  if($keys.Count -ne 1){return}
  $expected=Join-Path $keys[0].InstallLocation.Trim('"') 'harmony-work.exe'
  if([IO.Path]::GetFullPath($s.restartPath) -ne [IO.Path]::GetFullPath($expected)){return}
  if((Get-Item -LiteralPath $expected).VersionInfo.ProductName -ne 'Harmony Work'){return}
  if(!@(Get-Process harmony-work -ErrorAction SilentlyContinue|Where-Object {$_.Path -eq $expected}).Count){Start-Process -FilePath $expected}
 }catch{}
}
function ShowFailure([string]$message) {
 $controls.Heading.Text='更新未完成';$controls.StageText.Text=$message;$controls.PercentText.Text='';$controls.Progress.IsIndeterminate=$false;$controls.Progress.Value=0
 $controls.FailureActions.Visibility='Visible';$controls.HintText.Text='可以重试，或查看日志了解详情。'
}
$controls.DragRegion.Add_MouseLeftButtonDown({if($_.ChangedButton -eq 'Left'){$window.DragMove()}})
$controls.MinimizeButton.Add_Click({$window.WindowState='Minimized'})
$controls.RetryButton.Add_Click({try{StartWorker}catch{$script:displayState='failed';ShowFailure '无法启动更新，请查看日志。'}})
$controls.LogButton.Add_Click({$log=Join-Path $env:LOCALAPPDATA 'Harmony\WorkBuddy\updates\light-installer.log';if(Test-Path -LiteralPath $log){Start-Process notepad.exe -ArgumentList ('"'+$log+'"')}})
$controls.CloseButton.Add_Click({if(!$script:workerProcess -or $script:workerProcess.HasExited){SafeReturnToClient;$script:allowClose=$true;$window.Close()}})
$window.Add_Closing({if(!$script:allowClose){$_.Cancel=$true;if($script:workerProcess -and !$script:workerProcess.HasExited){$controls.HintText.Text='更新正在进行，请等待完成。'}else{SafeReturnToClient;$script:allowClose=$true;$window.Dispatcher.BeginInvoke([Action]{$window.Close()})|Out-Null}}})
$timer.Add_Tick({
 try {
  $s=ReadUpdateProgress $progressFile
  if($script:workerProcess -and $script:workerProcess.HasExited){
   if($s){$script:lastState=$s}
   if($script:workerProcess.ExitCode -eq 0 -and $s -and $s.phase -eq 'complete'){$script:result=0;$script:allowClose=$true;$timer.Stop();$window.Close()}
   elseif($script:displayState -ne 'failed'){$script:displayState='failed';ShowFailure $(if($s -and $s.phase -eq 'failed'){$s.detail}else{'更新进程意外结束，请查看日志后重试。'})}
   return
  }
  if($s){$script:lastState=$s;$controls.StageText.Text=$s.detail}
  if($s -and $null -ne $s.total -and $s.total -gt 0){$controls.Progress.IsIndeterminate=$false;$controls.Progress.Value=100*$s.completed/$s.total;$controls.PercentText.Text=[Math]::Floor(100*$s.completed/$s.total).ToString()+'%';$controls.HintText.Text=('{0:N0} / {1:N0} 项 · 完成后自动打开客户端' -f $s.completed,$s.total)}
  elseif(!$script:workerProcess -or !$script:workerProcess.HasExited){$controls.Progress.IsIndeterminate=$true;$controls.PercentText.Text='';$controls.HintText.Text='完成后将自动打开客户端，请暂时不要手动启动。'}

 }catch{if(!$script:workerProcess -or $script:workerProcess.HasExited){ShowFailure '无法读取更新状态，请查看日志后重试。'}}
})
$window.Add_ContentRendered({try{StartWorker;$timer.Start()}catch{$script:displayState='failed';ShowFailure '无法启动更新，请查看日志。'}})
try{$null=$window.ShowDialog()}finally{$timer.Stop();if($script:workerProcess){$script:workerProcess.Dispose()};Remove-Item -LiteralPath $progressFile -Force -ErrorAction SilentlyContinue}
exit $script:result
