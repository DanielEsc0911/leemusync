; LemuSync Inno Setup script (Inno Setup 6.3+). Layout: docs/specs/release.md (Install layout).
; Build from the repo root:
;   set LEMUSYNC_VERSION=0.0.1-alpha.1 & set LEMUSYNC_VERSION_NUMERIC=0.0.1
;   set LEMUSYNC_GUI_DIR=<flutter Release folder> & set LEMUSYNC_BIN_DIR=<dir with lemusync.exe + lemusyncd.exe>
;   iscc /O<output dir> packaging\windows\lemusync.iss

#define AppVersion GetEnv("LEMUSYNC_VERSION")
#define AppVersionNumeric GetEnv("LEMUSYNC_VERSION_NUMERIC")
#define GuiDir GetEnv("LEMUSYNC_GUI_DIR")
#define BinDir GetEnv("LEMUSYNC_BIN_DIR")

[Setup]
; AppId is permanent: never change it after the first release.
AppId={{350A21EC-E42B-4C95-8E97-B1433D66294D}
AppName=LemuSync
AppVersion={#AppVersion}
AppPublisher=LemuSync contributors
AppPublisherURL=https://github.com/DanielEsc0911/lemusync
VersionInfoVersion={#AppVersionNumeric}.0
DefaultDirName={autopf}\LemuSync
DefaultGroupName=LemuSync
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
LicenseFile=..\..\LICENSE
ChangesEnvironment=yes
WizardStyle=modern
UninstallDisplayIcon={app}\LemuSync.exe
OutputBaseFilename=lemusync-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"

[Files]
Source: "{#GuiDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#BinDir}\lemusync.exe"; DestDir: "{app}\bin"; Flags: ignoreversion
Source: "{#BinDir}\lemusyncd.exe"; DestDir: "{app}\bin"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\LemuSync"; Filename: "{app}\LemuSync.exe"; AppUserModelID: "io.github.danielesc0911.lemusync"

[Run]
Filename: "{app}\LemuSync.exe"; Description: "{cm:LaunchProgram,LemuSync}"; Flags: nowait postinstall skipifsilent

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
