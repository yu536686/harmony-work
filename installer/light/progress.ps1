function WriteUpdateProgress {
 param([string]$Path,[ValidateSet('preparing','waiting','verifying','staging','copying','starting','rollback','complete','failed')][string]$Phase,[string]$Detail,[Nullable[int]]$Completed=$null,[Nullable[int]]$Total=$null,[bool]$SafeRestart=$false,[string]$RestartPath='')
 if(!$Path){return}
 if($null -ne $Total -and ($Total -le 0 -or $null -eq $Completed -or $Completed -lt 0 -or $Completed -gt $Total)){throw 'Invalid progress counts'}
 if($null -eq $Total -and $null -ne $Completed){throw 'Progress counts require a total'}
 $state=@{schema=1;phase=$Phase;detail=$Detail;completed=$Completed;total=$Total;terminal=($Phase -in @('complete','failed'));safeRestart=($Phase -eq 'failed' -and $SafeRestart);restartPath=$RestartPath}
 $temporary=$Path+'.'+[guid]::NewGuid().ToString('N')+'.tmp'
 try {
  [IO.File]::WriteAllText($temporary,($state|ConvertTo-Json -Compress),(New-Object Text.UTF8Encoding($false)))
  if([IO.File]::Exists($Path)){[IO.File]::Replace($temporary,$Path,[NullString]::Value)}else{[IO.File]::Move($temporary,$Path)}
 } finally {if([IO.File]::Exists($temporary)){[IO.File]::Delete($temporary)}}
}
function ReadUpdateProgress([string]$Path) {
 $stream=$null;$reader=$null
 try {
  $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
  $reader=New-Object IO.StreamReader($stream,[Text.Encoding]::UTF8)
  $value=$reader.ReadToEnd()|ConvertFrom-Json
  if($value.schema -ne 1 -or $value.phase -notin @('preparing','waiting','verifying','staging','copying','starting','rollback','complete','failed')){return $null}
  return $value
 }catch{return $null}finally{if($reader){$reader.Dispose()}elseif($stream){$stream.Dispose()}}
}
