[CmdletBinding()]
param(
    [string]$VaultPath = "",
    [ValidateSet("", "sqlite", "postgres")]
    [string]$Backend = "",
    [string]$SqlitePath = "",
    [string]$DatabaseUrl = ""
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $scriptDir "..")).Path

if ([string]::IsNullOrWhiteSpace($VaultPath)) {
    $VaultPath = Join-Path $repoRoot "vault"
}

New-Item -ItemType Directory -Path $VaultPath -Force | Out-Null
$vaultFullPath = (Resolve-Path -LiteralPath $VaultPath).Path

if (-not [string]::IsNullOrWhiteSpace($Backend)) {
    $env:MNEMA_STORAGE_BACKEND = $Backend
}

if (-not [string]::IsNullOrWhiteSpace($SqlitePath)) {
    $sqliteParent = Split-Path -Parent $SqlitePath
    if (-not [string]::IsNullOrWhiteSpace($sqliteParent)) {
        New-Item -ItemType Directory -Path $sqliteParent -Force | Out-Null
    }
    $env:MNEMA_SQLITE_PATH = $SqlitePath
}

if (-not [string]::IsNullOrWhiteSpace($DatabaseUrl)) {
    $env:MNEMA_DATABASE_URL = $DatabaseUrl
}

Set-Location -LiteralPath $repoRoot
cargo run -p mnema-desktop -- gui --vault $vaultFullPath
exit $LASTEXITCODE
