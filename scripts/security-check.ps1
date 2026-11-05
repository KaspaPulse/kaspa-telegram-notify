$ErrorActionPreference = "Stop"

Write-Host "Kaspa Pulse - Security Check Pipeline" -ForegroundColor Cyan

function Run-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Block
    )

    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan

    $global:LASTEXITCODE = 0

    & $Block

    $exitCode = $LASTEXITCODE

    if ($null -ne $exitCode -and $exitCode -ne 0) {
        throw "$Name failed with exit code $exitCode"
    }

    Write-Host "OK: $Name" -ForegroundColor Green
}

Run-Step "cargo audit" {
    cargo audit
}

Run-Step "cargo deny check" {
    cargo deny check
}

Run-Step "cargo tree duplicate dependency scan" {
    cargo tree -d
}

Run-Step "cargo machete unused dependency scan" {
    cargo machete
}

Run-Step "cargo clippy strict warnings" {
    $env:SQLX_OFFLINE = "true"
    cargo clippy --all-targets --all-features -- -D warnings
}

Run-Step "cargo test" {
    $env:SQLX_OFFLINE = "true"
    $env:CARGO_INCREMENTAL = "0"
    cargo test
}

if (Test-Path "scripts\secret-scan.ps1") {
    Run-Step "secret scan" {
        powershell -ExecutionPolicy Bypass -File "scripts\secret-scan.ps1"
    }
}

if (Test-Path "scripts\admin-webhook-hardening-check.ps1") {
    Run-Step "admin webhook hardening check" {
        powershell -ExecutionPolicy Bypass -File "scripts\admin-webhook-hardening-check.ps1"
    }
}

if (Test-Path "scripts\rust-hardening-check.ps1") {
    Run-Step "rust hardening check" {
        powershell -ExecutionPolicy Bypass -File "scripts\rust-hardening-check.ps1"
    }
}

Write-Host ""
Write-Host "All supply-chain and security checks completed successfully." -ForegroundColor Green
