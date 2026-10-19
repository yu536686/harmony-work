Unicode true
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
LoadLanguageFile "${NSISDIR}\Contrib\Language files\SimpChinese.nlf"
Name "Harmony Work ${VERSION} Light Update"
OutFile "${OUTPUT}"
RequestExecutionLevel user
InstallDir ""
SilentInstall normal
SetCompressor /SOLID lzma
ShowInstDetails show
AutoCloseWindow true
Var Result
Var SilentArg
Var CheckArg
Var AutomaticArg
Var PowerShellExe
Function .onInit
 StrCpy $Result ""
 StrCpy $SilentArg ""
 IfSilent 0 +2
 StrCpy $SilentArg "-Silent"
 StrCpy $CheckArg ""
 ${GetParameters} $R0
 ClearErrors
 ${GetOptions} $R0 "/CHECK" $R1
 ${IfNot} ${Errors}
  StrCpy $CheckArg "-Check"
 ${EndIf}
 StrCpy $AutomaticArg ""
 ClearErrors
 ${GetOptions} $R0 "/UPDATE" $R1
 ${IfNot} ${Errors}
  StrCpy $AutomaticArg "-Automatic"
 ${EndIf}
 StrCpy $PowerShellExe "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
 ${If} ${RunningX64}
  StrCpy $PowerShellExe "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
 ${EndIf}
 ${If} $AutomaticArg != ""
 ${AndIf} $CheckArg == ""
  SetSilent normal
 ${EndIf}
FunctionEnd
Function .onGUIInit
 ${If} $AutomaticArg != ""
  GetDlgItem $0 $HWNDPARENT 2
  EnableWindow $0 0
 ${EndIf}
FunctionEnd
Function .onUserAbort
 ${If} $AutomaticArg != ""
 ${AndIf} $Result == ""
  Abort
 ${EndIf}
FunctionEnd
Section
 InitPluginsDir
 DetailPrint "正在准备更新。安装完成后会自动重启，请勿手动打开客户端。"
 SetOutPath "$PLUGINSDIR\payload"
 File /r "${PAYLOAD}\*"
 SetOutPath "$PLUGINSDIR"
 File "update.ps1"
 DetailPrint "正在检查运行环境并安装更新，请等待完成。"
 nsExec::ExecToLog '"$PowerShellExe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\update.ps1" -Payload "$PLUGINSDIR\payload" -InstallDirectory "$INSTDIR" $SilentArg $CheckArg $AutomaticArg'
 Pop $Result
 ${If} $Result != 0
  DetailPrint "更新未完成，请查看上方详情和更新日志。"
  SetAutoClose false
 ${Else}
  SetAutoClose true
 ${EndIf}
 SetErrorLevel $Result
SectionEnd
