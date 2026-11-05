$ErrorActionPreference = "Stop"

Write-Host "Kaspa Pulse - Local Secret Scan" -ForegroundColor Cyan

$findings = @()

$excludedExact = @(
    ".env",
    ".env.example",
    ".gitignore",
    "README.md",
    "DATABASE_SECURITY.md",
    "SECURITY_ADVISORIES.md",
    "SECURITY_CHECKLIST_ASVS.md",
    "scripts\secret-scan.ps1",
    "scripts\clean-git-history.ps1",
    "scripts\db-backup.ps1",
    "scripts\db-restore.ps1",
    "scripts\db-migrate.ps1"
)

function Get-RelativePath {
    param([string]$FullPath)

    $root = (Get-Location).Path
    $relative = $FullPath.Substring($root.Length).TrimStart("\", "/")
    return $relative.Replace("/", "\")
}

function Is-ExcludedExact {
    param([string]$RelativePath)

    foreach ($excluded in $excludedExact) {
        if ($RelativePath -ieq $excluded) {
            return $true
        }
    }

    return $false
}

function Is-IgnoredDirectory {
    param([string]$RelativePath)

    return (
        $RelativePath -match '^\.git\\' -or
        $RelativePath -match '^target\\' -or
        $RelativePath -match '^D:\\' -or
        $RelativePath -match '^\.sqlx\\'
    )
}

$files = Get-ChildItem -Recurse -File -ErrorAction SilentlyContinue

foreach ($file in $files) {
    $relative = Get-RelativePath $file.FullName

    if (Is-IgnoredDirectory $relative) {
        continue
    }

    if (Is-ExcludedExact $relative) {
        continue
    }

    if ($relative -match '^\.backup\\' -or $relative -match '^backups\\' -or $relative -match '^local-cleanup-backup-') {
        $findings += "Sensitive backup path present inside repository: $relative"
        continue
    }

    if ($file.Name -match '^project_code_export_.*\.txt$') {
        $findings += "Sensitive project export file present inside repository: $relative"
        continue
    }

    if ($file.Name -match '^repo-before-history-clean-.*\.bundle$') {
        $findings += "History-clean backup bundle present inside repository: $relative"
        continue
    }

    if ($file.Name -match '\.dump$' -or $file.Name -match '\.sql\.dump$' -or $file.Name -match '\.bak$') {
        $findings += "Database dump/backup file present inside repository: $relative"
        continue
    }

    $text = ""

    try {
        $text = Get-Content $file.FullName -Raw -ErrorAction Stop
    } catch {
        continue
    }

    if ($text -match 'BOT_TOKEN\s*[:=]\s*[0-9]{7,}:[A-Za-z0-9_-]{30,}') {
        $findings += "Real-looking Telegram BOT_TOKEN found in $relative"
    }

    if ($text -match 'DATABASE_URL\s*[:=]\s*["'']?postgres(?:ql)?://[^:\s]+:([^@\s"''`]+)@') {
        $password = $Matches[1]

        if ($password -notmatch '^(PUT_|YOUR_|APP_PASSWORD|TEST_PASSWORD|password|example|changeme|REDACTED|\*\*\*\*)') {
            $findings += "Real-looking DATABASE_URL password found in $relative"
        }
    }

    if ($text -match 'postgres(?:ql)?://postgres:([^@\s"''`]+)@') {
        $password = $Matches[1]

        if ($password -notmatch '^(PUT_|YOUR_|TEST_PASSWORD|password|example|changeme|REDACTED|\*\*\*\*)') {
            $findings += "Real-looking postgres superuser URL found in $relative"
        }
    }

    if ($text -match 'WEBHOOK_SECRET_TOKEN\s*[:=]\s*["'']?([A-Za-z0-9_-]{32,})') {
        $secret = $Matches[1]

        if ($secret -notmatch '^(PUT_|YOUR_|RANDOM_|SECRET_|TEST_|REDACTED)') {
            $findings += "Real-looking WEBHOOK_SECRET_TOKEN found in $relative"
        }
    }

    if ($text -match '-----BEGIN (RSA |OPENSSH |EC |PRIVATE )?PRIVATE KEY-----') {
        $findings += "Private key block found in $relative"
    }
}

if ($findings.Count -gt 0) {
    Write-Host "Secret scan findings:" -ForegroundColor Red
    $findings | ForEach-Object { Write-Host $_ -ForegroundColor Red }
    throw "Secret scan failed"
}

Write-Host "Secret scan passed." -ForegroundColor Green
