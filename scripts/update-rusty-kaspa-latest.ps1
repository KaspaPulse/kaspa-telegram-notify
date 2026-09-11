param(
    [string]$KaspaGitUrl = "https://github.com/kaspanet/rusty-kaspa.git",
    [switch]$AllowPrerelease,
    [switch]$Push,
    [switch]$NoBranch,
    [string]$BaseBranch = "dev"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false
Set-StrictMode -Version Latest

function Step($Name, [scriptblock]$Block) {
    Write-Host "`n==> $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    & $Block
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
    Write-Host "OK: $Name" -ForegroundColor Green
}

function Run-AllowFail($Name, [scriptblock]$Block) {
    Write-Host "`n==> $Name" -ForegroundColor Cyan
    $global:LASTEXITCODE = 0
    & $Block
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Host "FAILED: $Name exit code $code" -ForegroundColor Red
        return $false
    }
    Write-Host "OK: $Name" -ForegroundColor Green
    return $true
}

function ConvertTo-KaspaSemver {
    param([Parameter(Mandatory = $true)][string]$Tag)

    if ($Tag -notmatch '^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$') {
        return $null
    }

    $prerelease = $Matches[4]
    return [pscustomobject]@{
        Tag = $Tag
        Major = [int]$Matches[1]
        Minor = [int]$Matches[2]
        Patch = [int]$Matches[3]
        Prerelease = $prerelease
        StableRank = if ([string]::IsNullOrWhiteSpace($prerelease)) { 1 } else { 0 }
    }
}

function Compare-KaspaSemverCore {
    param(
        [Parameter(Mandatory = $true)]$Left,
        [Parameter(Mandatory = $true)]$Right
    )

    foreach ($property in @('Major', 'Minor', 'Patch')) {
        if ($Left.$property -gt $Right.$property) { return 1 }
        if ($Left.$property -lt $Right.$property) { return -1 }
    }

    if ($Left.StableRank -gt $Right.StableRank) { return 1 }
    if ($Left.StableRank -lt $Right.StableRank) { return -1 }
    return 0
}

function Get-LatestKaspaReleaseTag {
    param([switch]$AllowPrerelease)

    $tagsRaw = git ls-remote --tags --refs $KaspaGitUrl "refs/tags/v*"
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to list rusty-kaspa tags"
    }

    $tags = @()
    foreach ($line in $tagsRaw) {
        if ($line -notmatch 'refs/tags/(v[^\s]+)$') {
            continue
        }

        $candidate = ConvertTo-KaspaSemver -Tag $Matches[1]
        if ($null -eq $candidate) {
            continue
        }
        if (-not $AllowPrerelease -and $candidate.StableRank -eq 0) {
            continue
        }
        $tags += $candidate
    }

    if ($tags.Count -eq 0) {
        $kind = if ($AllowPrerelease) { "semver" } else { "stable semver" }
        throw "No $kind rusty-kaspa tags found."
    }

    $latest = $tags |
        Sort-Object `
            @{ Expression = 'Major'; Descending = $true },
            @{ Expression = 'Minor'; Descending = $true },
            @{ Expression = 'Patch'; Descending = $true },
            @{ Expression = 'StableRank'; Descending = $true },
            @{ Expression = 'Prerelease'; Descending = $true } |
        Select-Object -First 1

    return $latest.Tag
}

function Get-KaspaTagRevision {
    param([Parameter(Mandatory = $true)][string]$Tag)

    # Resolve both lightweight and annotated tags. For annotated tags, pin the peeled commit,
    # not the mutable/tag-object identity itself.
    $lines = @(git ls-remote --tags $KaspaGitUrl "refs/tags/$Tag" "refs/tags/$Tag^{}")
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to resolve rusty-kaspa tag '$Tag'."
    }

    $directRev = $null
    $peeledRev = $null
    foreach ($line in $lines) {
        if ($line -notmatch '^([0-9a-fA-F]{40})\s+(refs/tags/.+)$') {
            throw "Unexpected ls-remote result while resolving rusty-kaspa tag '$Tag'."
        }
        $revision = $Matches[1].ToLowerInvariant()
        $reference = $Matches[2]
        if ($reference -eq "refs/tags/$Tag") {
            $directRev = $revision
        } elseif ($reference -eq "refs/tags/$Tag^{}") {
            $peeledRev = $revision
        }
    }

    if ([string]::IsNullOrWhiteSpace($directRev)) {
        throw "No direct rusty-kaspa tag ref was found for '$Tag'."
    }

    return $(if ($peeledRev) { $peeledRev } else { $directRev })
}

function Get-CurrentKaspaPins {
    $cargo = Get-Content "Cargo.toml" -Raw -Encoding UTF8
    $dependencyNames = @(
        'kaspa-wrpc-client',
        'kaspa-rpc-core',
        'kaspa-addresses',
        'kaspa-consensus-core',
        'kaspa-hashes'
    )
    $sourcePattern = 'git\s*=\s*"https://github\.com/kaspanet/rusty-kaspa"'
    $allKaspaEntries = [regex]::Matches($cargo, $sourcePattern)
    if ($allKaspaEntries.Count -ne $dependencyNames.Count) {
        throw "Expected exactly $($dependencyNames.Count) direct rusty-kaspa dependencies; found $($allKaspaEntries.Count)."
    }

    $pins = @()
    foreach ($name in $dependencyNames) {
        $pattern = '(?m)^' + [regex]::Escape($name) +
            '\s*=\s*\{\s*version\s*=\s*"=([^"]+)"\s*,\s*' +
            'git\s*=\s*"https://github\.com/kaspanet/rusty-kaspa"\s*,\s*' +
            'rev\s*=\s*"([0-9a-fA-F]{40})"\s*\}\s*$'
        $match = [regex]::Match($cargo, $pattern)
        if (-not $match.Success) {
            throw "rusty-kaspa dependency '$name' is not pinned with exact version + immutable rev."
        }
        $pins += [pscustomobject]@{
            Name = $name
            Version = $match.Groups[1].Value
            Rev = $match.Groups[2].Value.ToLowerInvariant()
        }
    }

    return $pins
}

function Update-KaspaPins {
    param(
        [Parameter(Mandatory = $true)][string]$TargetTag,
        [Parameter(Mandatory = $true)][string]$TargetRev
    )

    if ($TargetTag -notmatch '^v(.+)$') {
        throw "Target rusty-kaspa tag '$TargetTag' does not have the required v-prefixed version."
    }
    $targetVersion = $Matches[1]
    if ($TargetRev -notmatch '^[0-9a-fA-F]{40}$') {
        throw "Target rusty-kaspa revision must be a full 40-character commit SHA."
    }
    $path = "Cargo.toml"
    $content = Get-Content $path -Raw -Encoding UTF8
    $new = $content
    foreach ($name in @('kaspa-wrpc-client', 'kaspa-rpc-core', 'kaspa-addresses', 'kaspa-consensus-core', 'kaspa-hashes')) {
        $pattern = '(?m)^(' + [regex]::Escape($name) +
            '\s*=\s*\{\s*version\s*=\s*")=[^"]+("\s*,\s*' +
            'git\s*=\s*"https://github\.com/kaspanet/rusty-kaspa"\s*,\s*' +
            'rev\s*=\s*")[0-9a-fA-F]{40}("\s*\}\s*)$'
        $replacement = '${1}=' + $targetVersion + '${2}' + $TargetRev.ToLowerInvariant() + '${3}'
        $updated = [regex]::Replace($new, $pattern, $replacement)
        if ($updated -eq $new) {
            throw "No exact-version/revision pin was changed for rusty-kaspa dependency '$name'."
        }
        $new = $updated
    }

    Set-Content $path $new -Encoding UTF8
}

Step "Verify clean working tree" {
    git status --short --branch
    $dirty = @(git status --porcelain)
    if ($dirty.Count -ne 0) {
        throw "Working tree is not clean. Refusing automated dependency mutation."
    }
}

Step "Find and verify rusty-kaspa release pin" {
    $script:LatestTag = Get-LatestKaspaReleaseTag -AllowPrerelease:$AllowPrerelease
    $script:LatestRev = Get-KaspaTagRevision -Tag $script:LatestTag
    Write-Host "Highest eligible rusty-kaspa tag: $script:LatestTag"
    Write-Host "Immutable revision for $script:LatestTag: $script:LatestRev"

    $script:CurrentPins = @(Get-CurrentKaspaPins)
    $currentVersions = @($script:CurrentPins.Version | Sort-Object -Unique)
    $currentRevisions = @($script:CurrentPins.Rev | Sort-Object -Unique)
    if ($currentVersions.Count -ne 1 -or $currentRevisions.Count -ne 1) {
        throw "rusty-kaspa dependencies are not pinned to one consistent exact version and revision."
    }

    $script:CurrentTag = "v$($currentVersions[0])"
    $script:CurrentRev = $currentRevisions[0]
    Write-Host "Current rusty-kaspa pin: $script:CurrentTag @ $script:CurrentRev"

    $currentVersion = ConvertTo-KaspaSemver -Tag $script:CurrentTag
    $latestVersion = ConvertTo-KaspaSemver -Tag $script:LatestTag
    if ($null -eq $currentVersion -or $null -eq $latestVersion) {
        throw "Unable to compare current and target rusty-kaspa versions safely."
    }
    if ((Compare-KaspaSemverCore -Left $currentVersion -Right $latestVersion) -gt 0) {
        throw "Refusing downgrade from $script:CurrentTag to $script:LatestTag."
    }
}

if ($script:CurrentTag -eq $script:LatestTag) {
    if ($script:CurrentRev -ne $script:LatestRev) {
        throw "SECURITY: upstream tag drift detected for $script:LatestTag. Current immutable pin is $script:CurrentRev but upstream now resolves to $script:LatestRev. Refusing automatic mutation; review manually."
    }
    Write-Host "`nAlready on highest eligible immutable rusty-kaspa pin: $script:LatestTag @ $script:LatestRev" -ForegroundColor Green
    exit 0
}

if (-not $NoBranch) {
    Step "Create update branch from checked-out base" {
        $currentBranch = (git branch --show-current).Trim()
        if ($currentBranch -ne $BaseBranch) {
            throw "Expected checked-out base branch '$BaseBranch' but found '$currentBranch'."
        }

        $safeTag = $script:LatestTag -replace '[^a-zA-Z0-9_.-]', '-'
        $branch = "auto/rusty-kaspa-$safeTag"
        $existing = git branch --list $branch
        if ($existing) {
            throw "Local update branch '$branch' already exists. Refusing to delete or overwrite it automatically."
        }

        git checkout -b $branch
        $script:UpdateBranch = $branch
        Write-Host "Update branch: $branch"
    }
} else {
    $script:UpdateBranch = (git branch --show-current).Trim()
    if ([string]::IsNullOrWhiteSpace($script:UpdateBranch)) {
        throw "No current branch is available for -NoBranch mode."
    }
}

Step "Update Cargo.toml rusty-kaspa immutable pins" {
    Update-KaspaPins -TargetTag $script:LatestTag -TargetRev $script:LatestRev
    Select-String -Path "Cargo.toml" -Pattern "kaspanet/rusty-kaspa|version =|rev ="
}

$env:SQLX_OFFLINE = "true"
$env:CARGO_INCREMENTAL = "0"
$env:RUST_BACKTRACE = "1"

Step "Refresh Cargo.lock with minimal resolver changes" {
    cargo check --all-targets --all-features
}

$allGood = $true

if (-not (Run-AllowFail "cargo fmt" { cargo fmt --all })) { $allGood = $false }
if (-not (Run-AllowFail "cargo check" { cargo check --locked --all-targets --all-features })) { $allGood = $false }
if (-not (Run-AllowFail "cargo clippy" { cargo clippy --locked --all-targets --all-features -- -D warnings })) { $allGood = $false }
if (-not (Run-AllowFail "cargo test" { cargo test --locked --all-targets --all-features })) { $allGood = $false }

if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
    if (-not (Run-AllowFail "cargo audit" { cargo audit })) { $allGood = $false }
} else {
    Write-Host "cargo-audit not installed; validation is incomplete." -ForegroundColor Yellow
    $allGood = $false
}

if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
    if (-not (Run-AllowFail "cargo deny check" { cargo deny check })) { $allGood = $false }
} else {
    Write-Host "cargo-deny not installed; validation is incomplete." -ForegroundColor Yellow
    $allGood = $false
}

if (Test-Path "scripts/security-check.ps1") {
    if (-not (Run-AllowFail "project security-check.ps1" {
        pwsh -NoProfile -File "scripts/security-check.ps1"
    })) { $allGood = $false }
}

Step "Show final diff" {
    git status --short --branch
    git diff --check
    git diff --stat
}

if (-not $allGood) {
    Write-Host "`nAutomatic Kaspa update failed validation. Do not merge or deploy." -ForegroundColor Red
    exit 1
}

if ($Push) {
    Step "Commit validated update" {
        git add Cargo.toml Cargo.lock
        $pending = @(git diff --cached --name-only)
        if ($pending.Count -eq 0) {
            throw "Validation passed but no update changes are staged."
        }
        git commit -m "chore(deps): update rusty-kaspa to $script:LatestTag"
    }

    Step "Push validated update branch" {
        git push --set-upstream origin $script:UpdateBranch
    }
}

Write-Host "`nAutomatic Kaspa update passed." -ForegroundColor Green
Write-Host "Branch: $script:UpdateBranch"
Write-Host "LatestTag: $script:LatestTag"
exit 0
