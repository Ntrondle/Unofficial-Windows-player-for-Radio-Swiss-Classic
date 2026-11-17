#ifndef AppVersion
  #define AppVersion "2.1.0"
#endif
[Setup]
AppId=io.github.nico.radio-swiss-classic-player.native
AppName=Radio Swiss Classic
AppVersion={#AppVersion}
AppPublisher=Nico
DefaultDirName={localappdata}\Programs\Radio Swiss Classic Native
DefaultGroupName=Radio Swiss Classic
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=RadioSwissClassicPlayer-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\RadioSwissClassic.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
DisableProgramGroupPage=yes
[Files]
Source: "..\dist\RadioSwissClassic.exe"; DestDir: "{app}"; Flags: ignoreversion
[Icons]
Name: "{group}\Radio Swiss Classic"; Filename: "{app}\RadioSwissClassic.exe"
[Run]
Filename: "{app}\RadioSwissClassic.exe"; Description: "Open Radio Swiss Classic"; Flags: nowait postinstall skipifsilent
