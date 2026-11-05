$ErrorActionPreference = "Stop"

Write-Host "Kaspa Pulse - Admin/Webhook/Telegram Hardening Check" -ForegroundColor Cyan

$requiredFiles = @(
    "src\presentation\telegram\handlers\admin_confirm.rs",
    "src\infrastructure\webhook_security.rs",
    "src\presentation\telegram\commands.rs",
    "src\presentation\telegram\menus.rs"
)

foreach ($file in $requiredFiles) {
    if (!(Test-Path $file)) {
        throw "Missing required file: $file"
    }
}

$adminConfirm = Get-Content "src\presentation\telegram\handlers\admin_confirm.rs" -Raw
foreach ($term in @("ADMIN_CONFIRM_TTL_SECS", "MuteAlerts", "UnmuteAlerts", "validate_admin_do_callback", "cleanup_expired")) {
    if ($adminConfirm -notmatch [regex]::Escape($term)) {
        throw "admin_confirm.rs missing: $term"
    }
}

$webhook = Get-Content "src\infrastructure\webhook_security.rs" -Raw
foreach ($term in @("WEBHOOK_SECRET_TOKEN", "WEBHOOK_BIND", "WEBHOOK_MAX_CONNECTIONS", "HEALTH_BIND", "metrics")) {
    if ($webhook -notmatch [regex]::Escape($term)) {
        throw "webhook_security.rs missing: $term"
    }
}

$commands = Get-Content "src\presentation\telegram\commands.rs" -Raw
foreach ($term in @("mute_alerts", "unmute_alerts", "alerts_status")) {
    if ($commands -notmatch [regex]::Escape($term)) {
        throw "commands.rs missing: $term"
    }
}

Write-Host "Admin/Webhook/Telegram hardening check passed." -ForegroundColor Green
