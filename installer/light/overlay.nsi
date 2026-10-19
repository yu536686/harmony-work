Unicode true
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
LoadLanguageFile "${NSISDIR}\Contrib\Language files\SimpChinese.nlf"
Name "Harmony Work ${VERSION} Light Update"
OutFile "${OUTPUT}"
RequestExecutionLevel user
InstallDir ""
SilentInstall silent
SetCompressor /SOLID lzma
ShowInstDetails show
AutoCloseWindow true
Page instfiles
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
FunctionEnd
Section
 InitPluginsDir
 DetailPrint "正在准备更新。安装完成后会自动重启，请勿手动打开客户端。"
 SetOutPath "$PLUGINSDIR\payload"
 File /r "${PAYLOAD}\*"
 SetOutPath "$PLUGINSDIR"
 File "update.ps1"
 File "progress.ps1"
 File "window.ps1"
 File "launcher.ps1"
 File "ProgressWindow.xaml"
 File /oname=brand.png "..\..\assets\branding\harmony-work-icon-original.png"
 DetailPrint "正在检查运行环境并安装更新，请等待完成。"
 nsExec::ExecToLog '"$PowerShellExe" -NoProfile -STA -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "$PLUGINSDIR\launcher.ps1" -Payload "$PLUGINSDIR\payload" -InstallDirectory "$INSTDIR" $SilentArg $CheckArg $AutomaticArg'
 Pop $Result
 SetErrorLevel $Result
SectionEnd
