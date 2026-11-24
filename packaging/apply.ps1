$ErrorActionPreference = 'Stop'
$target = $env:NEXTAI_TARGET
$stage = $env:NEXTAI_STAGE
$payload = Join-Path $stage 'update.exe'
$parentDir = Split-Path -LiteralPath $target
$swap = Join-Path $parentDir ('.nextai-' + [guid]::NewGuid() + '.tmp')
$backup = "$swap.bak"
try {
    $parent = Get-Process -Id ([int]$env:NEXTAI_PARENT) -ErrorAction SilentlyContinue
    if ($parent -and !$parent.WaitForExit(30000)) { throw 'The translator is still running.' }
    if ((Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash -ne $env:NEXTAI_HASH) { throw 'Update checksum mismatch.' }
    if ($env:NEXTAI_KIND -eq 'setup') {
        $result = Start-Process -FilePath $payload -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', ('/DIR="' + $parentDir + '"')) -PassThru -Wait
        if ($result.ExitCode -ne 0) { throw "Installer exited with code $($result.ExitCode)." }
    } elseif ($env:NEXTAI_KIND -eq 'portable') {
        [IO.File]::Copy($payload, $swap)
        [IO.File]::Replace($swap, $target, $backup)
    } else { throw 'Unknown package type.' }
    Start-Process -FilePath $target -WorkingDirectory $parentDir
    if ([IO.File]::Exists($backup)) { [IO.File]::Delete($backup) }
} catch {
    $message = $_.Exception.Message
    if ([IO.File]::Exists($backup)) { [IO.File]::Replace($backup, $target, $swap) }
    Add-Type -AssemblyName PresentationFramework
    [void][Windows.MessageBox]::Show($message, 'NextAI Translator - Update failed')
    exit 1
} finally {
    if ([IO.File]::Exists($swap)) { [IO.File]::Delete($swap) }
    Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
}
