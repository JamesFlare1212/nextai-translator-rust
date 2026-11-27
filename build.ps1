param([ValidateSet('x86_64-pc-windows-msvc', 'x86_64-pc-windows-gnu')][string]$Target = 'x86_64-pc-windows-msvc')
$ErrorActionPreference = 'Stop'
$targetDir = Join-Path ([IO.Path]::GetTempPath()) ('nextai-build-' + [guid]::NewGuid())
Push-Location $PSScriptRoot
try {
    $exe = cargo build --manifest-path ./Cargo.toml --target-dir $targetDir --locked --release --target $Target --bin nextai-translator --message-format=json-render-diagnostics |
        ForEach-Object { ConvertFrom-Json $_ } |
        Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'nextai-translator' -and $_.executable } |
        ForEach-Object { $_.executable }
    if ($LASTEXITCODE -ne 0 -or !$exe) { throw 'Application build failed' }
    $metadata = cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Could not read package version' }
    $version = ($metadata.packages | Where-Object name -eq 'nextai-translator').version
    ./packaging/build.ps1 -Version $version -Executable $exe -OutputDir dist
    Get-Item "dist/nextai-translator-$version-windows-x64-setup.exe", "dist/nextai-translator-$version-windows-x64-portable.exe"
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $targetDir) { Remove-Item -LiteralPath $targetDir -Recurse -Force }
}
