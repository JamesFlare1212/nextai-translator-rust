param([Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Executable, [Parameter(Mandatory)][string]$OutputDir)
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a stable version number' }
$exe = (Get-Item -LiteralPath $Executable).FullName
$out = (New-Item -ItemType Directory -Force $OutputDir).FullName
$tools = Join-Path ([IO.Path]::GetTempPath()) ('nextai-inno-' + [guid]::NewGuid())
try {
    $iscc = "${env:ProgramFiles(x86)}/Inno Setup 6/ISCC.exe"
    if (!(Test-Path -LiteralPath $iscc)) {
        New-Item -ItemType Directory $tools | Out-Null
        $bootstrap = Join-Path $tools 'innosetup.exe'
        Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_4/innosetup-6.7.4.exe' -OutFile $bootstrap
        if ((Get-FileHash $bootstrap).Hash -ne 'a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c') { throw 'Compiler checksum mismatch' }
        $p = Start-Process $bootstrap -ArgumentList "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /PORTABLE=1 /CURRENTUSER /DIR=`"$tools/compiler`"" -Wait -PassThru
        if ($p.ExitCode) { throw 'Could not unpack Inno Setup' }
        $iscc = Join-Path $tools 'compiler/ISCC.exe'
    }
    & $iscc "/O$out" "/DAppVersion=$Version" "/DAppExe=$exe" "$PSScriptRoot/windows.iss"
    if ($LASTEXITCODE) { throw 'Inno Setup compilation failed' }
    Copy-Item $exe "$out/nextai-translator-$Version-windows-x64-portable.exe"
} finally {
    if (Test-Path $tools) { Remove-Item -LiteralPath $tools -Recurse -Force }
}
