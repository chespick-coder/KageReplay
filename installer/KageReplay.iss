#define AppName "Kage Replay"
#define AppExe "KageReplay.exe"
#define AppGuid "{8C1F4E52-6A3B-4D7E-9F21-5B0C7A4E2D19}"
#define SetupVersion "1.0.0"
#ifndef DownloadBase
  #define DownloadBase "https://github.com/chespick-coder/KageReplay/releases/latest/download"
#endif

[Setup]
AppId={{#AppGuid}
AppName={#AppName}
AppVersion={#SetupVersion}
AppVerName={#AppName}
AppPublisher={#AppName}
VersionInfoVersion={#SetupVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} Setup
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
DisableReadyPage=yes
AllowRootDirectory=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist\release
OutputBaseFilename=KageReplay-Setup
SetupIconFile=build\kage.ico
WizardSmallImageFile=build\small-58.png,build\small-116.png
WizardStyle=modern dynamic
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/ultra64
SolidCompression=yes
CloseApplications=no
RestartApplications=no
ShowLanguageDialog=no
SetupLogging=yes

[Languages]
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Messages]
ru.SetupWindowTitle=Установка - %1
ru.UninstallAppFullTitle=Удаление - %1

[CustomMessages]
ru.NoMediaFoundation=В этой версии Windows нет компонентов мультимедиа (Media Foundation), без них Kage Replay не может записывать видео и звук.%n%nТак бывает в выпусках Windows N. Установите «Пакет компонентов мультимедиа»: Параметры > Приложения > Дополнительные компоненты > Добавить компонент > «Пакет компонентов мультимедиа», затем перезагрузите компьютер и запустите установку снова.
ru.Downloading=Скачивание Kage Replay: %1 из %2 МБ
ru.DownloadFailed=Не удалось скачать Kage Replay. Проверьте подключение к интернету и запустите установку ещё раз.%n%n%1
ru.RemoveUserData=Удалить также настройки, журналы и кеш повторов Kage Replay?%n%nЗаписанные видео и скриншоты останутся на месте.

[ISSigKeys]
Name: release; KeyFile: "signing\release.pub"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{tmp}\{#AppExe}"; DestDir: "{app}"; ExternalSize: 25000000; Flags: external ignoreversion
Source: "THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser
Filename: "{app}\{#AppExe}"; Parameters: "--background"; Flags: nowait runasoriginaluser; Check: WizardSilent and not IsUpdate
Filename: "{app}\{#AppExe}"; Flags: nowait runasoriginaluser; Check: IsUpdate

[Code]
var
  AppStopped: Boolean;
  AppInstalled: Boolean;

function IsUpdate: Boolean;
begin
  Result := ExpandConstant('{param:UPDATE|0}') = '1';
end;

function AppRunning: Boolean;
begin
  Result := CheckForMutexes('Local\KageReplay.Daemon,Local\KageReplay.UI,Local\KageReplay.Overlay');
end;

procedure StopKageReplay(ExePath: String);
var
  ResultCode: Integer;
  Waited: Integer;
begin
  if FileExists(ExePath) then
    Exec(ExePath, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Waited := 0;
  while AppRunning and (Waited < 20000) do
  begin
    Sleep(250);
    Waited := Waited + 250;
  end;
  if AppRunning then
  begin
    Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM {#AppExe}', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(500);
  end;
end;

function Megabytes(Bytes: Int64): String;
begin
  Result := Format('%.1f', [Bytes / 1048576.0]);
end;

function OnDownloadProgress(const Url, FileName: String; const Progress, ProgressMax: Int64): Boolean;
begin
  if ProgressMax > 0 then
    WizardForm.PreparingLabel.Caption := FmtMessage(CustomMessage('Downloading'), [Megabytes(Progress), Megabytes(ProgressMax)]);
  Result := True;
end;

function DownloadApp: String;
begin
  Result := '';
  WizardForm.PreparingLabel.Visible := True;
  try
    DownloadTemporaryFileWithISSigVerify('{#DownloadBase}/{#AppExe}', '', '{#AppExe}', nil, @OnDownloadProgress);
  except
    Log('download failed: ' + GetExceptionMessage);
    Result := FmtMessage(CustomMessage('DownloadFailed'), [GetExceptionMessage]);
  end;
end;

function InstalledVersion: String;
var
  Version: String;
begin
  Result := '';
  if GetVersionNumbersString(ExpandConstant('{app}\{#AppExe}'), Version) then
  begin
    if Copy(Version, Length(Version) - 1, 2) = '.0' then
      Delete(Version, Length(Version) - 1, 2);
    Result := Version;
  end;
end;

function InitializeSetup: Boolean;
begin
  Result := FileExists(ExpandConstant('{sys}\mfplat.dll'));
  if not Result then
    SuppressibleMsgBox(CustomMessage('NoMediaFoundation'), mbCriticalError, MB_OK, IDOK);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := DownloadApp;
  if Result = '' then
  begin
    AppStopped := AppRunning;
    StopKageReplay(ExpandConstant('{app}\{#AppExe}'));
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Version: String;
begin
  if CurStep = ssPostInstall then
  begin
    AppInstalled := True;
    Version := InstalledVersion;
    if Version <> '' then
      RegWriteStringValue(HKA, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppGuid}_is1', 'DisplayVersion', Version);
  end;
end;

procedure DeinitializeSetup;
var
  ResultCode: Integer;
begin
  if AppStopped and not AppInstalled and FileExists(ExpandConstant('{app}\{#AppExe}')) then
    ExecAsOriginalUser(ExpandConstant('{app}\{#AppExe}'), '--background', '', SW_SHOWNORMAL, ewNoWait, ResultCode);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopKageReplay(ExpandConstant('{app}\{#AppExe}'));
  if CurUninstallStep = usPostUninstall then
  begin
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'KageReplay');
    if not UninstallSilent then
      if MsgBox(CustomMessage('RemoveUserData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        DelTree(ExpandConstant('{userappdata}\KageReplay'), True, True, True);
        DelTree(ExpandConstant('{localappdata}\KageReplay'), True, True, True);
      end;
  end;
end;
