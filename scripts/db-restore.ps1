param(
    [Parameter(Mandatory = $true)]
    [string]$BackupFile
)

$ErrorActionPreference = "Stop"

if (!(Test-Path ".env")) {
    throw ".env not found"
}

if (!(Test-Path $BackupFile)) {
    throw "Backup file not found: $BackupFile"
}

$envText = Get-Content ".env" -Raw
$match = [regex]::Match($envText, "(?m)^DATABASE_URL=(.*)$")

if (!$match.Success) {
    throw "DATABASE_URL not found in .env"
}

$databaseUrl = $match.Groups[1].Value.Trim()

Write-Host "Restore target is DATABASE_URL from .env." -ForegroundColor Yellow
Write-Host "This script uses pg_restore --clean --if-exists --no-owner." -ForegroundColor Yellow

pg_restore --clean --if-exists --no-owner --dbname=$databaseUrl $BackupFile

if ($LASTEXITCODE -ne 0) {
    throw "pg_restore failed"
}

Write-Host "Database restore completed." -ForegroundColor Green
