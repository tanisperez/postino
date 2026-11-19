; Inno Setup 6 script for Postino. Compile with ISCC, defines on the command line:
;   ISCC.exe /DAppVersion=0.1.0-rc.1 /DSourceExe=C:\path\postino.exe /DOutputDir=C:\out postino.iss
;   AppVersion  full version, may carry a pre-release suffix (X.Y.Z or X.Y.Z-rc.1)
;   SourceExe   path to the release postino.exe
;   OutputDir   folder for Postino-<version>-windows-x86_64-setup.exe (default: .\Output)
; The LICENSE and the icon are found relative to this file (repo root and crates/postino-app).

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef SourceExe
  #error Pass /DSourceExe=<path to postino.exe>
#endif
#ifndef OutputDir
  #define OutputDir "Output"
#endif

; VersionInfoVersion must be numeric: keep X.Y.Z, drop any "-prerelease", append ".0".
#define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)

#define AppName "Postino"
#define AppExe "postino.exe"
#define AppId "codes.tanis.postino"
#define Publisher "Estanislao Pérez Nartallo"
#define AppURL "https://postino.tanis.codes"

[Setup]
; Never change this GUID, it identifies the installed app for upgrades and the uninstaller.
; The doubled brace is Inno's escape for a literal one.
AppId={{3DD5886A-368E-44E6-A2DE-84B7B3405749}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
AppUpdatesURL={#AppURL}
VersionInfoVersion={#NumericVersion}.0
VersionInfoProductVersion={#NumericVersion}.0
VersionInfoCompany={#Publisher}
VersionInfoDescription={#AppName} setup
VersionInfoProductName={#AppName}
; Per user install, no UAC. With PrivilegesRequired=lowest, {autopf} resolves to
; {localappdata}\Programs, so it is the same folder as writing it out; explicit is clearer.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\{#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
; The addtopath task puts the install folder in the user's Path (#284, #298): tell running
; programs, so new terminals see it. The uninstaller broadcasts the removal too.
ChangesEnvironment=yes
; The updater runs the installer silently while Postino may be open: close it through the
; Restart Manager, never ask for a reboot, and relaunch from [Run].
CloseApplications=yes
RestartApplications=no
SetupIconFile=..\..\crates\postino-app\assets\postino.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir={#OutputDir}
OutputBaseFilename=Postino-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; Inno Setup bundles no Galician translation, so there is no Galician here.
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"

[CustomMessages]
english.AddToPath=Add Postino to the PATH (run "postino" from a terminal)
spanish.AddToPath=Añadir Postino al PATH (ejecutar "postino" desde una terminal)
italian.AddToPath=Aggiungi Postino al PATH (esegui "postino" da un terminale)
english.Integration=Integration:
spanish.Integration=Integración:
italian.Integration=Integrazione:

; Upgrades and the updater's silent installs keep the previous choice (UsePreviousTasks).
[Tasks]
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:Integration}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

; The app sets the explicit AppUserModelID codes.tanis.postino, so the shortcuts must carry the
; same one or taskbar pinning and grouping break.
[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppId}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppId}"; Tasks: desktopicon

; HKA is HKCU here (lowest privileges), so nothing needs admin.
[Registry]
Root: HKA; Subkey: "Software\Classes\.postino"; ValueType: string; ValueName: ""; ValueData: "Postino.Request"; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.postino\OpenWithProgids"; ValueType: string; ValueName: "Postino.Request"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\Postino.Request"; ValueType: string; ValueName: ""; ValueData: "Postino request"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Postino.Request\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\Postino.Request\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".postino"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""

; Interactively, a checkbox on the last page. Silent installs (winget) do not launch Postino,
; except when the in-app updater asks for it with /relaunch=1.
[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: RelaunchRequested

[Code]
function RelaunchRequested: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;

// The user's Path (HKCU, no admin), so `postino` works in any terminal (#284). Kept as
// REG_EXPAND_SZ, since it usually holds %USERPROFILE% style entries.
const
  EnvironmentKey = 'Environment';

function PathHasDir(Paths, Dir: String): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

procedure AddToPath(Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Paths) then
    Paths := '';
  // The updater runs the installer on every update: add the folder once.
  if PathHasDir(Paths, Dir) then
    Exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(HKCU, EnvironmentKey, 'Path', Paths) then
    Exit;
  if not PathHasDir(Paths, Dir) then
    Exit;
  Paths := ';' + Paths + ';';
  StringChangeEx(Paths, ';' + Dir + ';', ';', True);
  // Drop the separators added above, and the one left where the entry was last.
  while (Length(Paths) > 0) and (Paths[1] = ';') do
    Delete(Paths, 1, 1);
  while (Length(Paths) > 0) and (Paths[Length(Paths)] = ';') do
    Delete(Paths, Length(Paths), 1);
  RegWriteExpandStringValue(HKCU, EnvironmentKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
