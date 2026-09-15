<#
.SYNOPSIS
    Development setup for be\ on Windows.

.DESCRIPTION
    The Windows counterpart of scripts/setup.sh — same steps, same order, same
    output. If this and docs/setup-js.md disagree, the script is right.

    NOT YET TESTED ON WINDOWS. Treat the first run as a review.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Be = Join-Path $RepoRoot "be"

function Write-Step($m) { Write-Host "`n==> $m" }
function Write-Log($m)  { Write-Host "  $m" }
function Write-Warn($m) { Write-Host "  ! $m" -ForegroundColor Yellow }

Push-Location $Be
try {
    $want = (Get-Content ".nvmrc" -Raw).Trim()

    # --- 1. the Node you develop with ---------------------------------------
    Write-Step "Node version"
    $have = try { (& node --version 2>$null) } catch { "none" }
    Write-Log "want (be\.nvmrc): $want"
    Write-Log "have (on PATH):   $have"
    if ($have -ne $want) {
        Write-Warn "Mismatch. Dev/production drift is the classic 'works on my machine'."
        Write-Warn "Fix with:  nvm install $want; nvm use $want"
        Write-Warn "Continuing — the bundled runtime below is the one that ships."
    }

    # --- 2. the Node that ships ----------------------------------------------
    Write-Step "Private runtime (be\runtime)"
    & (Join-Path $RepoRoot "scripts\install-node.ps1") -Dest (Join-Path $Be "runtime")
    if ($LASTEXITCODE -ne 0) { throw "runtime install failed" }

    # --- 3. dependencies ------------------------------------------------------
    Write-Step "Dependencies"
    if (Test-Path "package-lock.json") {
        Write-Log "npm ci (exact lockfile)"
        & npm ci
    } else {
        Write-Log "no lockfile yet — npm install, which creates one. Commit it."
        & npm install
    }
    if ($LASTEXITCODE -ne 0) { throw "dependency install failed" }

    # --- 4. local configuration ----------------------------------------------
    Write-Step "Configuration"
    if (Test-Path ".env") {
        Write-Log ".env exists — left untouched"
    } else {
        Copy-Item ".env.example" ".env"
        Write-Log "created .env from .env.example"
    }
    $envText = Get-Content ".env" -Raw
    $missing = @()
    foreach ($line in Get-Content ".env.example") {
        if ($line -match '^([A-Z_][A-Z0-9_]*)=') {
            if ($envText -notmatch "(?m)^$($Matches[1])=") { $missing += $Matches[1] }
        }
    }
    if ($missing.Count -gt 0) {
        Write-Warn "keys in .env.example missing from .env: $($missing -join ' ')"
    }

    # --- 5. prove it works ----------------------------------------------------
    Write-Step "Verifying"
    Write-Log "typecheck"; & npm run --silent typecheck
    Write-Log "tests"
    & npm test --silent 2>$null | Out-Null
    if ($LASTEXITCODE -eq 0) { Write-Log "  pass" } else { Write-Log "  (no tests yet)" }

    Write-Log "run against the bundled runtime"
    & (Join-Path $Be "runtime\bin\node.exe") --env-file-if-exists=.env "src\main.ts" |
        ForEach-Object { Write-Host "    $_" }

    Write-Step "Ready"
    Write-Log "npm run dev            watch mode, ambient node"
    Write-Log "npm run start:sealed   run against the bundled runtime (what users get)"
} finally {
    Pop-Location
}
