param(
    [Parameter(Mandatory = $true)]
    [string]$BinDir
)

$projectDir = Split-Path -Parent $PSScriptRoot
Set-Location $projectDir

cargo build --release -p zed-vertical-align-lsp
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Copy-Item -Force `
    (Join-Path $projectDir 'target\release\zed-vertical-align-lsp.exe') `
    (Join-Path $BinDir 'zed-vertical-align-lsp.exe')
