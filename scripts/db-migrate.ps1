$ErrorActionPreference = "Stop"

if (!(Test-Path ".env")) {
    throw ".env not found"
}

if (!(Test-Path "migrations")) {
    throw "migrations folder not found"
}

$envText = Get-Content ".env" -Raw
$match = [regex]::Match($envText, "(?m)^DATABASE_URL=(.*)$")

if (!$match.Success) {
    throw "DATABASE_URL not found in .env"
}

$databaseUrl = $match.Groups[1].Value.Trim()

Get-ChildItem "migrations" -Filter "*.sql" | Sort-Object Name | ForEach-Object {
    Write-Host "Applying migration: $($_.Name)" -ForegroundColor Cyan

    psql $databaseUrl -v ON_ERROR_STOP=1 -f $_.FullName

    if ($LASTEXITCODE -ne 0) {
        throw "Migration failed: $($_.Name)"
    }
}

Write-Host "All migrations applied." -ForegroundColor Green
