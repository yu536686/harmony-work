param([string]$InstallerDirectory=(Join-Path (Split-Path -Parent $PSScriptRoot) 'installer\light'))
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
Add-Type -AssemblyName PresentationFramework
$source=$InstallerDirectory
$fixture=Join-Path $env:TEMP ('HarmonyLauncherTest-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory "$fixture\payload" -Force|Out-Null
foreach($name in @('launcher.ps1','progress.ps1','window.ps1','ProgressWindow.xaml')){Copy-Item "$source\$name" $fixture}
Copy-Item (Join-Path $source '..\..\assets\branding\harmony-work-icon-original.png') "$fixture\brand.png"
[IO.File]::WriteAllText("$fixture\payload\payload.manifest.json",'{"version":"1.1.2-rc.1"}')
$stub=@'
param($Payload,$InstallDirectory,[switch]$Silent,$ProgressFile,[switch]$Automatic)
. (Join-Path $PSScriptRoot 'progress.ps1')
WriteUpdateProgress -Path $ProgressFile -Phase verifying -Detail '校验测试' -Completed 1 -Total 4
Start-Sleep -Seconds 2
if(!(Test-Path "$PSScriptRoot\retried")){
 Set-Content "$PSScriptRoot\retried" 'first attempt complete'
 WriteUpdateProgress -Path $ProgressFile -Phase failed -Detail '测试失败，可以重试' -SafeRestart $false
 exit 1
}
WriteUpdateProgress -Path $ProgressFile -Phase complete -Detail '测试完成'
exit 0
'@
[IO.File]::WriteAllText("$fixture\update.ps1",$stub,(New-Object Text.UTF8Encoding($true)))
$global:HarmonyProgressTestState=@{seenFailure=$false;seenCounts=$false;closeBlocked=$false;timedOut=$false}
$start=Get-Date
$watch=New-Object Windows.Threading.DispatcherTimer;$watch.Interval=[TimeSpan]::FromMilliseconds(100)
$watch.Add_Tick({
 $w=@([Windows.PresentationSource]::CurrentSources|ForEach-Object {$_.RootVisual}|Where-Object {$_ -is [Windows.Window] -and $_.Title -eq 'Harmony Work 更新'})|Select-Object -First 1
 if(((Get-Date)-$start).TotalSeconds -gt 25){[Environment]::Exit(9)}
 if(!$w){return}
 if($w.FindName('PercentText').Text -eq '25%'){
  $global:HarmonyProgressTestState.seenCounts=$true
  if(!$global:HarmonyProgressTestState.closeBlocked){$w.Close();$global:HarmonyProgressTestState.closeBlocked=$w.IsVisible}
 }
 if($w.FindName('FailureActions').Visibility -eq 'Visible' -and !$global:HarmonyProgressTestState.seenFailure){
  $global:HarmonyProgressTestState.seenFailure=$true
  $w.FindName('RetryButton').RaiseEvent((New-Object Windows.RoutedEventArgs([Windows.Controls.Button]::ClickEvent)))
 }
 if(((Get-Date)-$start).TotalSeconds -gt 25){$global:HarmonyProgressTestState.timedOut=$true;$watch.Stop();[Environment]::Exit(9)}
})
try{
 $watch.Start()
 & "$fixture\launcher.ps1" -Payload "$fixture\payload" -InstallDirectory 'C:\fixture' -Automatic -Silent
 $watch.Stop()
 if(!$global:HarmonyProgressTestState.seenFailure -or !$global:HarmonyProgressTestState.seenCounts -or !$global:HarmonyProgressTestState.closeBlocked -or $LASTEXITCODE -ne 0){throw "Lifecycle failed failure=$global:HarmonyProgressTestState.seenFailure counts=$global:HarmonyProgressTestState.seenCounts blocked=$global:HarmonyProgressTestState.closeBlocked exit=$LASTEXITCODE"}
 'PASS real WPF window shows stage percentage and blocks closing during work'
 'PASS failure exposes retry and reruns worker only after its exit'
 'PASS verified worker success closes branded window automatically'
}finally{$watch.Stop();Remove-Item $fixture -Recurse -Force -ErrorAction SilentlyContinue}
