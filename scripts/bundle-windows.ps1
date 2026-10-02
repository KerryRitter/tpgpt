$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo build --release --locked -p tpgpt
if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
python scripts/third-party-notices.py
if ($LASTEXITCODE -ne 0) { throw 'Dependency notice generation failed' }
$BuildRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$BundleRoot = Join-Path (Get-Location) 'native/bundle'
$Stage = Join-Path $BundleRoot 'windows-portable'
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
$App = Join-Path $Stage 'TPGPT'
New-Item -ItemType Directory -Force $App | Out-Null
Copy-Item (Join-Path $BuildRoot 'release/tpgpt.exe') $App
Copy-Item README.md $App
Copy-Item (Join-Path $BundleRoot 'THIRD-PARTY-NOTICES.txt') $App
Compress-Archive -Path $App -DestinationPath (Join-Path $BundleRoot 'tpgpt-windows-x86_64.zip') -Force
Write-Output "Bundle: $BundleRoot/tpgpt-windows-x86_64.zip"
