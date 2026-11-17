$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    cargo build --locked --release
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    New-Item -ItemType Directory -Force dist | Out-Null
    Copy-Item target/release/radio-swiss-classic.exe dist/RadioSwissClassic.exe -Force
    Compress-Archive -Path dist/RadioSwissClassic.exe -DestinationPath dist/RadioSwissClassic-native-windows-x64.zip -Force
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"').Matches[0].Groups[1].Value
    $compiler = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'
    if (!(Test-Path $compiler)) { throw 'Install Inno Setup 6 to build the installer (choco install innosetup).' }
    & $compiler "/DAppVersion=$version" windows/installer.iss
    if ($LASTEXITCODE -ne 0) { throw 'Installer build failed' }
    Write-Host "Built portable executable, ZIP, and per-user installer in dist/"
} finally {
    Pop-Location
}
