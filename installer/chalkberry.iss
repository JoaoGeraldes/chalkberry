; Inno Setup script for Chalkberry (https://jrsoftware.org/isinfo.php).
; Build the app first (cargo build --release), then compile this file with
; Inno Setup. The installer is written to installer\output\.

#define AppName "Chalkberry"
; Keep in step with `version` in Cargo.toml.
#define AppVersion "0.1.0"
#define AppExe "chalkberry.exe"

[Setup]
; Never change: this is how a newer setup finds and upgrades the installed app.
AppId={{6E0B6C3A-2F5D-4B8E-9C1A-7D4E3F2A1B90}
AppName={#AppName}
AppVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Installs for the current user only, so no administrator rights needed.
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.18362
OutputDir=output
OutputBaseFilename=ChalkberrySetup-{#AppVersion}
UninstallDisplayIcon={app}\{#AppExe}
SetupIconFile=..\assets\chalkberry.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "startup"; Description: "Start {#AppName} when Windows starts"; Flags: unchecked
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
; The GPL requires its text to go out with every copy of the app.
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\TRADEMARKS.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon
Name: "{userstartup}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: startup

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName} now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Close the app first, so its file can be removed.
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im {#AppExe}"; Flags: runhidden; RunOnceId: "StopChalkberry"
