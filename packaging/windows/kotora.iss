; Kotora for Windows: the installer (Inno Setup 6.3 or newer).
;
; Built by packaging\windows\build.ps1 (the windows workflow runs it):
;   iscc /DAppVersion=0.2.1 /DFileVersion=0.2.1.1 /DRelease=0.2.1 /DSourceDir=<staged files> /DOutputDir=<out> kotora.iss
;
; What it does, all for the current user and without admin rights:
;   - installs into %LOCALAPPDATA%\Programs\Kotora (or the folder picked; /DIR= when silent);
;   - a Start menu shortcut that carries Kotora's app id (taskbar grouping, toasts), and a
;     desktop shortcut when asked;
;   - adds the folder to the user's PATH, so `kot` works in a terminal (a task, on by default);
;   - asks a running Kotora to quit first (kot quit), when updating and when uninstalling;
;   - the uninstaller removes the programs, the shortcuts, the PATH entry, the start-at-login
;     entry and the app id's registration. It KEEPS the user's settings, sign-in and saved list
;     (%APPDATA%\kotora, %LOCALAPPDATA%\kotora) unless the user says to delete them (a question,
;     default No; when silent only with /REMOVEDATA).
;
; Silent use (winget, the CI smoke test):
;   Kotora-...-setup.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART [/DIR="C:\x"] [/TASKS="addtopath,desktopicon"]
;   unins000.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART [/REMOVEDATA]
;
; SIGNING (not yet: the programs and the installer are unsigned). The hook for SignPath, or any
; signtool: sign kotora.exe and kot.exe before this script runs (build.ps1, "Sign-Files"), and
; let Inno sign the installer and its uninstaller by giving iscc a sign tool:
;   iscc "/Ssignpath=<command> $f" ... and uncomment the next two lines.
;SignTool=signpath
;SignedUninstaller=yes

#ifndef AppVersion
  #error Pass /DAppVersion=<version> (see packaging\windows\build.ps1)
#endif
#ifndef FileVersion
  #define FileVersion AppVersion + ".0"
#endif
#ifndef Release
  #define Release AppVersion
#endif
#ifndef SourceDir
  #define SourceDir "..\..\dist"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist-out"
#endif

; The same id as the app's (app/src/main.rs APP_ID): the taskbar and the toasts go by it.
#define AppUserModelId "io.github.hanami117.kotora"

[Setup]
; Never change this id: it is how an update finds the installed copy.
AppId={{17091F1F-7829-4B92-964B-D2AA5E246767}
AppName=Kotora
AppVersion={#AppVersion}
AppVerName=Kotora {#AppVersion}
AppPublisher=Kotora
AppPublisherURL=https://kotora.moe
AppSupportURL=https://github.com/hanami117/kotora/issues
AppUpdatesURL=https://github.com/hanami117/kotora/releases
AppCopyright=Copyright (C) 2026 Thomas Lonigro. GPL-3.0-or-later
VersionInfoVersion={#FileVersion}
VersionInfoProductName=Kotora
VersionInfoDescription=Kotora Setup
; Per user, no admin: {autopf} is %LOCALAPPDATA%\Programs here.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Kotora
DisableProgramGroupPage=yes
DisableDirPage=auto
UsePreviousAppDir=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir={#OutputDir}
OutputBaseFilename=Kotora-{#Release}-windows-x64-setup
SetupIconFile=kotora.ico
UninstallDisplayIcon={app}\kotora.exe
UninstallDisplayName=Kotora
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
InfoBeforeFile=installer-note.txt
; The PATH task changes the user's environment: tell running programs.
ChangesEnvironment=yes
; A running Kotora is asked to quit by the code below; this is the fallback for files in use.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add kot, Kotora's command line, to your PATH"; GroupDescription: "Command line:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\kotora.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\kot.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README-WINDOWS.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Kotora"; Filename: "{app}\kotora.exe"; AppUserModelID: "{#AppUserModelId}"; Comment: "Anime tracker"
Name: "{autodesktop}\Kotora"; Filename: "{app}\kotora.exe"; AppUserModelID: "{#AppUserModelId}"; Comment: "Anime tracker"; Tasks: desktopicon

[Run]
Filename: "{app}\kotora.exe"; Description: "Start Kotora"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvKey = 'Environment';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  AppIdKey = 'Software\Classes\AppUserModelId\{#AppUserModelId}';

{ The user's PATH without Dir (compared without case, a trailing backslash ignored). }
function PathWithout(const Path, Dir: String): String;
var
  Rest, Item, Want: String;
  P: Integer;
begin
  Result := '';
  Want := Lowercase(RemoveBackslashUnlessRoot(Dir));
  Rest := Path;
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    if P = 0 then
    begin
      Item := Rest;
      Rest := '';
    end
    else
    begin
      Item := Copy(Rest, 1, P - 1);
      Rest := Copy(Rest, P + 1, Length(Rest));
    end;
    if (Trim(Item) <> '') and (Lowercase(RemoveBackslashUnlessRoot(Trim(Item))) <> Want) then
    begin
      if Result <> '' then
        Result := Result + ';';
      Result := Result + Item;
    end;
  end;
end;

{ Dir is one of the entries of Path (compared without case, a trailing backslash ignored). }
function IsOnPath(const Path, Dir: String): Boolean;
var
  All, Want: String;
begin
  All := ';' + Lowercase(Path) + ';';
  Want := Lowercase(RemoveBackslashUnlessRoot(Dir));
  Result := (Pos(';' + Want + ';', All) > 0) or (Pos(';' + Want + '\;', All) > 0);
end;

{ Adds Dir to the user's PATH (once, at the end) or takes it out. Only HKCU, never the system's.
  A PATH that needs no change is left exactly as it is. }
procedure SetOnPath(const Dir: String; Add: Boolean);
var
  Old, New: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Old) then
    Old := '';
  if IsOnPath(Old, Dir) = Add then
    exit;
  if Add then
  begin
    New := Old;
    if (New <> '') and (New[Length(New)] <> ';') then
      New := New + ';';
    New := New + Dir;
  end
  else
    New := PathWithout(Old, Dir);
  if New = '' then
    RegDeleteValue(HKEY_CURRENT_USER, EnvKey, 'Path')
  else
    RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', New);
end;

{ Asks a running Kotora to quit (the installed kot does it) and waits until it is gone, at most
  about 10 seconds. With no Kotora running, kot says so at once. }
procedure QuitKotora(const Dir: String);
var
  Kot: String;
  Code, I: Integer;
begin
  Kot := AddBackslash(Dir) + 'kot.exe';
  if not FileExists(Kot) then
    exit;
  if not Exec(Kot, 'quit', '', SW_HIDE, ewWaitUntilTerminated, Code) then
    exit;
  for I := 1 to 20 do
  begin
    { `kot status` fails once nothing answers. }
    if not Exec(Kot, 'status', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
      break;
    Sleep(500);
  end;
  { The program's file is free a moment after its pipe goes. }
  Sleep(500);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  QuitKotora(ExpandConstant('{app}'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SetOnPath(ExpandConstant('{app}'), WizardIsTaskSelected('addtopath'));
end;

{ /REMOVEDATA on the uninstaller's command line. }
function RemoveDataAsked(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/REMOVEDATA') = 0 then
      Result := True;
end;

function InitializeUninstall(): Boolean;
begin
  Result := True;
  QuitKotora(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Remove: Boolean;
begin
  if CurUninstallStep = usUninstall then
  begin
    SetOnPath(ExpandConstant('{app}'), False);
    { What the app itself wrote for the system: start at login, and the toasts' name and icon. }
    RegDeleteValue(HKEY_CURRENT_USER, RunKey, 'Kotora');
    RegDeleteValue(HKEY_CURRENT_USER, ApprovedKey, 'Kotora');
    RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, AppIdKey);
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    { The user's own data stays unless they say otherwise. }
    Remove := RemoveDataAsked();
    if (not Remove) and (not UninstallSilent()) then
      Remove := MsgBox('Also delete your Kotora settings, your sign-in and the saved copy of your list from this PC?' + #13#10 + #13#10 +
        'Your list on MyAnimeList or AniList is not touched. Choose No to keep everything for the next time you install Kotora.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
    if Remove then
    begin
      DelTree(ExpandConstant('{userappdata}\kotora'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\kotora'), True, True, True);
    end;
  end;
end;
