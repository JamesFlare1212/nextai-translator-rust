[Setup]
AppId=NextAI-Translator-Rust
AppName=NextAI Translator
AppVersion={#AppVersion}
AppPublisher=NextAI Translator
DefaultDirName={localappdata}\Programs\NextAI Translator
DefaultGroupName=NextAI Translator
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
WizardStyle=modern
OutputDir=..\release-assets
OutputBaseFilename=nextai-translator-{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\nextai.ico
UninstallDisplayIcon={app}\nextai-translator.exe
Compression=lzma2
SolidCompression=yes
[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "nextai-translator.exe"; Flags: ignoreversion
Source: "nextai-setup.marker"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
[Icons]
Name: "{group}\NextAI Translator"; Filename: "{app}\nextai-translator.exe"
[Run]
Filename: "{app}\nextai-translator.exe"; Description: "Launch NextAI Translator"; Flags: nowait postinstall skipifsilent
