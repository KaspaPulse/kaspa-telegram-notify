$ErrorActionPreference = "Stop"

Write-Host "Kaspa Pulse - Git history sensitive artifact cleanup" -ForegroundColor Cyan
Write-Host "WARNING: This rewrites Git history and force-pushes dev and main." -ForegroundColor Yellow

if (!(Test-Path "Cargo.toml")) {
    throw "Run from repository root."
}

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$bundle = "repo-before-history-clean-$timestamp.bundle"

git bundle create $bundle --all
Write-Host "Created full backup bundle: $bundle" -ForegroundColor Green

git filter-branch --force --index-filter "git rm -r --cached --ignore-unmatch .backup backups *.dump" --prune-empty --tag-name-filter cat -- --all

git reflog expire --expire=now --all
git gc --prune=now --aggressive

git push origin dev --force-with-lease
git push origin main --force-with-lease

Write-Host "History cleanup completed. Ask GitHub support or wait for GC if sensitive blobs remain cached remotely." -ForegroundColor Green
