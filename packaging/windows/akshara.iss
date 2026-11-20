; Akshara Office: per-user Windows installer (Inno Setup 6).
; Installs into %LOCALAPPDATA%\Programs\Akshara without administrator rights
; and registers file associations for the current user only (HKCU).
; Build with packaging/windows/build-installer.ps1 (fetches missing tools),
; build-installer.sh (Git Bash), or: iscc akshara.iss

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef BuildDir
  #define BuildDir "..\..\app\build\windows\x64\runner\Release"
#endif

[Setup]
AppId={{6C1E8B9A-4F2D-4C7E-9A1B-2B7D5E9F3A10}
AppName=Akshara Office
AppVersion={#AppVersion}
AppPublisher=Akshara contributors
AppPublisherURL=https://example.invalid/akshara
DefaultDirName={localappdata}\Programs\Akshara
DefaultGroupName=Akshara Office
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=akshara-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\app\windows\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\akshara.exe
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "associate"; Description: "Open OpenDocument files (.odt, .ods, .odp, .odg, .odf) with Akshara"; GroupDescription: "File associations:"

[Files]
Source: "{#BuildDir}\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\NOTICE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Akshara Office"; Filename: "{app}\akshara.exe"
Name: "{autodesktop}\Akshara Office"; Filename: "{app}\akshara.exe"; Tasks: desktopicon

[Registry]
; Per-user file associations (HKCU\Software\Classes), no admin needed.
Root: HKCU; Subkey: "Software\Classes\Akshara.Document"; ValueType: string; ValueName: ""; ValueData: "Akshara Office document"; Flags: uninsdeletekey; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\Akshara.Document\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\akshara.exe,0"; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\Akshara.Document\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\akshara.exe"" ""%1"""; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.odt\OpenWithProgids"; ValueType: string; ValueName: "Akshara.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.ods\OpenWithProgids"; ValueType: string; ValueName: "Akshara.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.odf\OpenWithProgids"; ValueType: string; ValueName: "Akshara.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.odp\OpenWithProgids"; ValueType: string; ValueName: "Akshara.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.odg\OpenWithProgids"; ValueType: string; ValueName: "Akshara.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

[Run]
Filename: "{app}\akshara.exe"; Description: "{cm:LaunchProgram,Akshara Office}"; Flags: nowait postinstall skipifsilent
