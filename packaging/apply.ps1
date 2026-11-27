$ErrorActionPreference = 'Stop'
$target = $env:NEXTAI_TARGET
$stage = $env:NEXTAI_STAGE
$payload = Join-Path $stage 'update.exe'
$parentDir = Split-Path -LiteralPath $target
try {
    $parent = Get-Process -Id ([int]$env:NEXTAI_PARENT) -ErrorAction SilentlyContinue
    if ($parent -and !$parent.WaitForExit(30000)) { throw 'The translator is still running.' }
    if ((Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash -ne $env:NEXTAI_HASH) { throw 'Update checksum mismatch.' }
    $result = Start-Process -FilePath $payload -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', ('/DIR="' + $parentDir + '"')) -PassThru -Wait
    if ($result.ExitCode -ne 0) { throw "Installer exited with code $($result.ExitCode)." }
    Start-Process -FilePath $target -WorkingDirectory $parentDir
} catch {
    Add-Type -AssemblyName PresentationFramework
    [void][Windows.MessageBox]::Show($_.Exception.Message, 'NextAI Translator - Update failed')
    exit 1
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
}
