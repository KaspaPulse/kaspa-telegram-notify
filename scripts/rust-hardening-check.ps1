$ErrorActionPreference = "Stop"

Write-Host "Kaspa Pulse - Rust Hardening Check" -ForegroundColor Cyan

if (!(Test-Path "src")) {
    throw "src directory not found"
}

# Match real Rust unsafe usage only, not the word "unsafe" inside error messages.
$unsafeFindings = Select-String `
    -Path "src\*.rs","src\**\*.rs" `
    -Pattern "^\s*unsafe\s+fn\b|^\s*unsafe\s+impl\b|^\s*unsafe\s*\{|=\s*unsafe\s*\{" `
    -ErrorAction SilentlyContinue

if ($unsafeFindings) {
    Write-Host "Unsafe Rust usage found:" -ForegroundColor Red
    $unsafeFindings | ForEach-Object {
        Write-Host "$($_.Path):$($_.LineNumber): $($_.Line)" -ForegroundColor Red
    }
    throw "Unsafe Rust usage requires explicit review"
}

$unwrapFindings = Select-String `
    -Path "src\*.rs","src\**\*.rs" `
    -Pattern "\.unwrap\(\)|\.expect\(" `
    -ErrorAction SilentlyContinue |
    Where-Object {
        $_.Path -notmatch "\\tests\\" -and
        $_.Line -notmatch "unwrap_or|unwrap_or_else|unwrap_or_default" -and
        $_.Line -notmatch "expect\(`"env test lock poisoned`"\)"
    }

if ($unwrapFindings) {
    Write-Host "unwrap()/expect() usage found in production source:" -ForegroundColor Yellow
    $unwrapFindings | ForEach-Object {
        Write-Host "$($_.Path):$($_.LineNumber): $($_.Line)" -ForegroundColor Yellow
    }
    throw "Production unwrap()/expect() requires explicit review"
}

Write-Host "Rust hardening check passed." -ForegroundColor Green
