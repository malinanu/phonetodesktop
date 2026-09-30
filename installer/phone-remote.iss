; Built by CI: iscc /DVersion=1.2.3 installer\phone-remote.iss
#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId={{6B6D5F0E-6E0B-4B85-9C55-1F8E7C2B7A11}
AppName=Phone Remote
AppVersion={#Version}
AppPublisher=Phone Remote
DefaultDirName={autopf}\Phone Remote
DefaultGroupName=Phone Remote
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=PhoneRemote-Setup-{#Version}
Compression=lzma2
SolidCompression=yes
UninstallDisplayIcon={app}\phone-remote.exe
CloseApplications=force
WizardStyle=modern

[Files]
Source: "..\agent\target\release\phone-remote.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Phone Remote"; Filename: "{app}\phone-remote.exe"

[Run]
; Allow phones on private networks to reach the agent (any port, this program only).
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Phone Remote"""; Flags: runhidden
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Phone Remote"" dir=in action=allow protocol=TCP profile=private program=""{app}\phone-remote.exe"""; Flags: runhidden
Filename: "{app}\phone-remote.exe"; Description: "Start Phone Remote and pair a phone"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/im phone-remote.exe /f"; Flags: runhidden; RunOnceId: "KillAgent"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Phone Remote"""; Flags: runhidden; RunOnceId: "DelFirewall"
Filename: "{sys}\reg.exe"; Parameters: "delete HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v PhoneRemote /f"; Flags: runhidden; RunOnceId: "DelAutostart"
