Unicode true
!include "FileFunc.nsh"
!include "LogicLib.nsh"
Name "Harmony Work ${VERSION} Light Update"
OutFile "${OUTPUT}"
RequestExecutionLevel user
InstallDir ""
SilentInstall normal
SetCompressor /SOLID lzma
ShowInstDetails show
Var Result
Var SilentArg
Section
 InitPluginsDir
 SetOutPath "$PLUGINSDIR\payload"
 File /r "${PAYLOAD}\*"
 SetOutPath "$PLUGINSDIR"
 File "update.ps1"
 StrCpy $SilentArg ""
 IfSilent 0 +2
 StrCpy $SilentArg "-Silent"
 nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\update.ps1" -Payload "$PLUGINSDIR\payload" -InstallDirectory "$INSTDIR" $SilentArg'
 Pop $Result
 SetErrorLevel $Result
SectionEnd
