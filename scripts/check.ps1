# Everything, in one command.
#
# This exists because a subset quietly became "the tests". The Rust crates and
# the Node backend have separate runners, and for a whole session only one of
# them was being run — nothing failed, nothing warned, and the launcher's suite
# simply was not being checked. A command that covers both is the fix; a
# reminder to remember is not.
#
# Unix twin: scripts/check.sh. Change one, change the other.
$ErrorActionPreference = "Continue"

Set-Location (Join-Path $PSScriptRoot "..")
$failed = @()

function Step {
    param([string]$Name, [scriptblock]$Body)
    Write-Host ""
    Write-Host "== $Name ==" -ForegroundColor White
    & $Body
    if ($LASTEXITCODE -eq 0) {
        Write-Host "ok  $Name" -ForegroundColor Green
    } else {
        Write-Host "FAILED  $Name" -ForegroundColor Red
        $script:failed += $Name
    }
}

# Rust: one workspace, so this covers fe, launcher and shared together.
Step "cargo test --workspace" { cargo test --workspace --quiet }
Step "cargo clippy"           { cargo clippy --workspace --all-targets --quiet }

# Node: its own runner, which is the half that went unchecked.
Step "be: npm test"           { npm --prefix be test }
Step "be: typecheck"          { npm --prefix be run typecheck }

# Generated files — see the note in check.sh. One implementation in
# fe/scripts/css-check.mjs, called from both twins, so this pair cannot drift
# the way three copies of the target-directory rule did.
Step "fe: stylesheet"         { npm --prefix fe run --silent css:check }

Write-Host ""
if ($failed.Count -eq 0) {
    Write-Host "All checks passed." -ForegroundColor Green
    exit 0
}
Write-Host "$($failed.Count) check(s) failed:" -ForegroundColor Red
$failed | ForEach-Object { Write-Host "  - $_" }
exit 1
