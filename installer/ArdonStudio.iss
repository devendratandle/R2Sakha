; ═══════════════════════════════════════════════════════════════════════
;  Ardon Studio — Windows installer (Inno Setup 6)
;
;  Ardon Studio runs Ardon-R2 as its engine (`r2 --ide`), so the installer
;  puts an r2.exe built with `--ide` beside ArdonStudio.exe, where the app
;  looks first. It installs for the current user, with no admin rights.
;
;  Build (from the repository root):
;    1. cargo build --release -p r2sakha
;    2. copy an r2.exe with --ide to installer\bundle\r2.exe
;    3. "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\ArdonStudio.iss
;  Output: installer\Output\ArdonStudio-Setup-<version>.exe
; ═══════════════════════════════════════════════════════════════════════

#define AppExe   AddBackslash(SourcePath) + "..\target\release\ArdonStudio.exe"
#define R2Exe    AddBackslash(SourcePath) + "bundle\r2.exe"
#if !FileExists(AppExe)
  #error ArdonStudio.exe is not built: cargo build --release -p r2sakha
#endif
#if !FileExists(R2Exe)
  #error installer\bundle\r2.exe is missing: copy an r2.exe built with --ide there
#endif
; the version is the workspace's, as the exe carries it
#define MyAppVersion   GetStringFileInfo(AppExe, "FileVersion")
#define R2Version      GetStringFileInfo(R2Exe, "FileVersion")
#define MyAppName      "Ardon Studio"
#define MyAppPublisher "Devendra Tandale"
#define MyAppURL       "https://github.com/devendratandle/R2Sakha"

[Setup]
AppId={{6C1E9A52-3B7D-4E0F-9A84-2D5C7F31B9E6}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=Output
OutputBaseFilename=ArdonStudio-Setup-{#MyAppVersion}
SetupIconFile=r2.ico
UninstallDisplayIcon={app}\ArdonStudio.exe
UninstallDisplayName={#MyAppName} {#MyAppVersion}
ChangesAssociations=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assoc"; Description: "Open .sakha analyses and .r2proj projects with Ardon Studio"; GroupDescription: "File associations:"

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
; the engine, beside the app: Ardon-R2 {#R2Version} with r2 --ide
Source: "{#R2Exe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\ArdonStudio.exe"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\ArdonStudio.exe"; Tasks: desktopicon

[Registry]
; HKA is the current user's classes for a per-user install, the machine's for an admin one
Root: HKA; Subkey: "Software\Classes\.sakha"; ValueType: string; ValueData: "ArdonStudio.Analysis"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Analysis"; ValueType: string; ValueData: "Sakha analysis"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Analysis\DefaultIcon"; ValueType: string; ValueData: "{app}\ArdonStudio.exe,0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Analysis\shell\open\command"; ValueType: string; ValueData: """{app}\ArdonStudio.exe"" ""%1"""; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.r2proj"; ValueType: string; ValueData: "ArdonStudio.Project"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Project"; ValueType: string; ValueData: "Ardon Studio project"; Flags: uninsdeletekey; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Project\DefaultIcon"; ValueType: string; ValueData: "{app}\ArdonStudio.exe,0"; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\ArdonStudio.Project\shell\open\command"; ValueType: string; ValueData: """{app}\ArdonStudio.exe"" ""%1"""; Tasks: assoc

[Run]
Filename: "{app}\ArdonStudio.exe"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent
