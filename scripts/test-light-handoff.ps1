param([Parameter(Mandatory=$true)][string]$RunningProductExe)
$ErrorActionPreference='Stop'
if(![Environment]::Is64BitProcess){throw 'Run this regression test with 64-bit Windows PowerShell'}
$installer=Join-Path (Split-Path -Parent $PSScriptRoot) 'installer\light\update.ps1'
$source=Get-Content -LiteralPath $installer -Raw
$from=$source.IndexOf('function Progress(');$to=$source.IndexOf('$changed=@()')
if($from -lt 0 -or $to -le $from){throw 'Installer function boundary not found'}
$fixture=Join-Path $env:TEMP ('HarmonyUpdateHandoffTest-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $fixture|Out-Null
$log=Join-Path $fixture 'test.log';$handle=$null
try {
 . ([ScriptBlock]::Create($source.Substring($from,$to-$from)))
 $before=(Get-FileHash -LiteralPath $RunningProductExe -Algorithm SHA256).Hash
 if(!@(GetRunningProduct $RunningProductExe).Count){throw 'The supplied client must be running for this regression'}
 try{WaitForProductExit $RunningProductExe 0;throw 'Live client was missed'}catch{if($_.Exception.Message -notlike '*still running*'){throw}}
 Write-Output 'PASS running client prevents overwrite'
 $target=Join-Path $fixture 'target.exe'
 [IO.File]::WriteAllText($target,'unchanged acceptance fixture')
 $hash=(Get-FileHash -LiteralPath $target).Hash
 WaitForProductExit $target 0;WaitForInstallAccess $target 0
 Write-Output 'PASS closed target is writable'
 $handle=[IO.File]::Open($target,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
 try{WaitForInstallAccess $target 0;throw 'Locked target was missed'}catch{if($_.Exception.Message -notlike '*still in use*'){throw}}
 $handle.Dispose();$handle=$null
 if((Get-FileHash -LiteralPath $target).Hash -ne $hash){throw 'Lock probe changed bytes'}
 Write-Output 'PASS file lock prevents replacement without changing bytes'
 $start=New-Object Diagnostics.ProcessStartInfo
 $start.FileName="$env:WINDIR\SysWOW64\WindowsPowerShell\v1.0\powershell.exe"
 $start.Arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$installer+'" -Payload "'+$fixture+'" -Check -Silent'
 $start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $start.EnvironmentVariables['TEMP']=$fixture
 $child=[Diagnostics.Process]::Start($start)
 $null=$child.StandardOutput.ReadToEnd();$null=$child.StandardError.ReadToEnd();$child.WaitForExit()
 $code=$child.ExitCode;$child.Dispose()
 $guard=Get-Content -LiteralPath (Join-Path $fixture 'HarmonyWorkUpdateCheck\light-installer.log') -Raw
 if($code -eq 0 -or $guard -notlike '*requires 64-bit PowerShell*'){throw '32-bit script host was not rejected before installation'}
 if((Get-FileHash -LiteralPath $RunningProductExe -Algorithm SHA256).Hash -ne $before){throw 'Regression changed current client'}
 Write-Output 'PASS 32-bit script host fails before modifying installed client'
}finally{if($handle){$handle.Dispose()};Remove-Item -LiteralPath $fixture -Recurse -Force}
