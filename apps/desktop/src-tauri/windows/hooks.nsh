; Installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; Registers Novero Mail as an email app, so Windows lists it under
; Settings > Apps > Default apps for email links (mailto:). Windows doesn't let
; apps make themselves the default; the person picks it there, and Settings in
; the app has a button that opens that page.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\NoveroMail.mailto" "" "Novero Mail email link"
  WriteRegStr SHCTX "Software\Classes\NoveroMail.mailto" "URL Protocol" ""
  WriteRegStr SHCTX "Software\Classes\NoveroMail.mailto\DefaultIcon" "" "$INSTDIR\${MAINBINARYNAME}.exe,0"
  WriteRegStr SHCTX "Software\Classes\NoveroMail.mailto\shell\open\command" "" '"$INSTDIR\${MAINBINARYNAME}.exe" "%1"'
  WriteRegStr SHCTX "Software\NoveroMail\Capabilities" "ApplicationName" "${PRODUCTNAME}"
  WriteRegStr SHCTX "Software\NoveroMail\Capabilities" "ApplicationDescription" "Email for Gmail, Outlook.com and any IMAP account"
  WriteRegStr SHCTX "Software\NoveroMail\Capabilities\URLAssociations" "mailto" "NoveroMail.mailto"
  WriteRegStr SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}" "Software\NoveroMail\Capabilities"
  ; Tells Explorer the associations changed (SHCNE_ASSOCCHANGED).
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue SHCTX "Software\RegisteredApplications" "${PRODUCTNAME}"
  DeleteRegKey SHCTX "Software\NoveroMail"
  DeleteRegKey SHCTX "Software\Classes\NoveroMail.mailto"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
