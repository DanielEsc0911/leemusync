; LeemuSync Inno Setup script (Inno Setup 6.3+). Layout: docs/specs/release.md (Install layout).
; Build from the repo root:
;   set LEEMUSYNC_VERSION=0.0.1-alpha.1 & set LEEMUSYNC_VERSION_NUMERIC=0.0.1
;   set LEEMUSYNC_GUI_DIR=<flutter Release folder> & set LEEMUSYNC_BIN_DIR=<dir with leemusync.exe + leemusyncd.exe>
;   iscc /O<output dir> packaging\windows\leemusync.iss

#define AppVersion GetEnv("LEEMUSYNC_VERSION")
#define AppVersionNumeric GetEnv("LEEMUSYNC_VERSION_NUMERIC")
#define GuiDir GetEnv("LEEMUSYNC_GUI_DIR")
#define BinDir GetEnv("LEEMUSYNC_BIN_DIR")

[Setup]
; AppId is permanent: never change it after the first release.
AppId={{350A21EC-E42B-4C95-8E97-B1433D66294D}
AppName=LeemuSync
AppVersion={#AppVersion}
AppPublisher=LeemuSync contributors
AppPublisherURL=https://github.com/DanielEsc0911/leemusync
VersionInfoVersion={#AppVersionNumeric}.0
DefaultDirName={autopf}\LeemuSync
DefaultGroupName=LeemuSync
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
LicenseFile=..\..\LICENSE
ChangesEnvironment=yes
WizardStyle=modern
UninstallDisplayIcon={app}\LeemuSync.exe
OutputBaseFilename=leemusync-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[Files]
Source: "{#GuiDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#BinDir}\leemusync.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#BinDir}\leemusyncd.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\LeemuSync"; Filename: "{app}\LeemuSync.exe"; AppUserModelID: "io.github.danielesc0911.leemusync"

[Run]
Filename: "{app}\LeemuSync.exe"; Description: "{cm:LaunchProgram,LeemuSync}"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

function PathContains(Paths, Dir: string): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

procedure AddToPath(Dir: string);
var
  Paths: string;
begin
  if not RegQueryStringValue(HKLM, EnvKey, 'Path', Paths) then
    Paths := '';
  if PathContains(Paths, Dir) then
    exit;
  if (Paths <> '') and (Copy(Paths, Length(Paths), 1) <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(HKLM, EnvKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(Dir: string);
var
  Paths: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKLM, EnvKey, 'Path', Paths) then
    exit;
  Paths := ';' + Paths + ';';
  P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Paths));
  while P > 0 do
  begin
    Delete(Paths, P, Length(Dir) + 1);
    P := Pos(';' + Uppercase(Dir) + ';', Uppercase(Paths));
  end;
  Paths := Copy(Paths, 2, Length(Paths) - 2);
  RegWriteExpandStringValue(HKLM, EnvKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    AddToPath(ExpandConstant('{app}\bin'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}\bin'));
end;
