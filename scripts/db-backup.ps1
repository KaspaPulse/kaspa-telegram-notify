$ErrorActionPreference = "Stop"

if (!(Test-Path ".env")) {
    throw ".env not found"
}

$envText = Get-Content ".env" -Raw
$match = [regex]::Match($envText, "(?m)^DATABASE_URL=(.*)$")

if (!$match.Success) {
    throw "DATABASE_URL not found in .env"
}

$databaseUrl = $match.Groups[1].Value.Trim()
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$backupDir = "backups\db"

New-Item -ItemType Directory -Path $backupDir -Force | Out-Null

$outFile = Join-Path $backupDir "kaspa-pulse-$timestamp.dump"

pg_dump $databaseUrl --format=custom --file=$outFile

if ($LASTEXITCODE -ne 0) {
    throw "pg_dump failed"
}

Write-Host "Database backup created: $outFile" -ForegroundColor Green
