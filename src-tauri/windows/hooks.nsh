!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ${IfNot} ${Silent}
      MessageBox MB_ICONEXCLAMATION|MB_OK "如需清理程序数据并保留工作区，请使用 Harmony Work 独立卸载器。此卸载入口不执行深度清理。"
    ${EndIf}
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend
