$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ("NextAI smoke " + [guid]::NewGuid())
$env:NEXTAI_STAGE = Join-Path $root 'stage'
$env:NEXTAI_TARGET = Join-Path $root 'translator.exe'
$env:NEXTAI_KIND = 'portable'
$env:NEXTAI_PARENT = '2147483647'
try {
    New-Item -ItemType Directory $env:NEXTAI_STAGE -Force | Out-Null
    Copy-Item "$env:SystemRoot/System32/hostname.exe" $env:NEXTAI_TARGET
    Copy-Item "$env:SystemRoot/System32/whoami.exe" "$env:NEXTAI_STAGE/update.exe"
    $env:NEXTAI_HASH = (Get-FileHash "$env:NEXTAI_STAGE/update.exe").Hash
    $script = (Resolve-Path "$PSScriptRoot/apply.ps1").Path
    $helper = Start-Process powershell.exe -ArgumentList "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$script`"" -PassThru -WindowStyle Hidden
    if (!$helper.WaitForExit(20000)) { $helper.Kill(); throw 'Portable update timed out' }
    if ($helper.ExitCode -ne 0 -or (Get-FileHash $env:NEXTAI_TARGET).Hash -ne $env:NEXTAI_HASH -or (Test-Path $env:NEXTAI_STAGE)) { throw 'Portable replacement failed' }
    Write-Output 'Portable replacement passed'
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
