[CmdletBinding()]
param(
    [string]$OutputDir = "",
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $scriptDir "..")).Path

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $repoRoot "dist\mnema-desktop-windows"
}

Set-Location -LiteralPath $repoRoot

if (-not $NoBuild) {
    cargo build --release -p mnema-desktop
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

$exePath = Join-Path $repoRoot "target\release\mnema-desktop.exe"
if (-not (Test-Path -LiteralPath $exePath)) {
    throw "Release executable not found: $exePath"
}

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
Copy-Item -LiteralPath $exePath -Destination (Join-Path $OutputDir "mnema-desktop.exe") -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "README.md") -Destination $OutputDir -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "README-ja.md") -Destination $OutputDir -Force
Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination $OutputDir -Force

$launcherPath = Join-Path $OutputDir "run-mnema.ps1"
$launcher = @'
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$vault = Join-Path $root "vault"
New-Item -ItemType Directory -Path $vault -Force | Out-Null
& (Join-Path $root "mnema-desktop.exe") gui --vault $vault
exit $LASTEXITCODE
'@
Set-Content -LiteralPath $launcherPath -Value $launcher -Encoding UTF8

Write-Host "Packaged Mnema desktop to $OutputDir"
