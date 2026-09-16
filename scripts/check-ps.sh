#!/usr/bin/env bash
#
# Parse and lint every .ps1 in scripts/, using a PowerShell that this machine
# does not otherwise have.
#
#   scripts/check-ps.sh
#
# Why it is not part of check.sh. CLAUDE.md has said for a while that the
# PowerShell scripts here "parse and lint clean", and until this existed
# nothing in the tree could establish that: there is no pwsh, no powershell and
# no PSScriptAnalyzer on this machine, so the claim rested on nothing runnable.
# It is still kept out of check.sh on purpose, because it needs Docker and the
# network — check.sh gates every rn-land, and a step that fails when the
# network is down would make landing depend on something unrelated to the
# change. Run this when a .ps1 changes; that is the only time it can tell you
# anything new.
#
# What it can and cannot say. The container runs PowerShell 7 on Linux, while a
# stock Windows box runs Windows PowerShell 5.1 — so a 5.1-only syntax error
# could still get through, and anything Windows-only (WinForms, the registry,
# scheduled tasks) is not merely unlinted but unrunnable. Parsing and static
# analysis are the whole of what is on offer here. They are not nothing: the
# analyzer caught a null-on-the-right comparison in install-gui.ps1 that would
# have misbehaved silently.
set -euo pipefail

IMAGE=mcr.microsoft.com/powershell:latest
ANALYZER=1.22.0     # the current release refuses PowerShell below 7.4.6; the image is 7.4.2
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { printf '\nERROR: %s\n' "$*" >&2; exit 1; }

command -v docker >/dev/null || die "this needs docker, which is not on PATH"
docker info >/dev/null 2>&1 || die "docker is installed but not usable as this user"

work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
cat > "$work/run.ps1" <<'INNER'
$ErrorActionPreference = "Stop"
$failed = 0

Write-Host "== parse =="
Get-ChildItem -Path /repo/scripts -Filter *.ps1 | Sort-Object Name | ForEach-Object {
    $errors = $null; $tokens = $null
    [System.Management.Automation.Language.Parser]::ParseFile($_.FullName, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors -and $errors.Count) {
        $script:failed = 1
        Write-Host ("  FAIL  {0}  ({1} error(s))" -f $_.Name, $errors.Count)
        $errors | Select-Object -First 5 | ForEach-Object {
            Write-Host ("          line {0}: {1}" -f $_.Extent.StartLineNumber, $_.Message)
        }
    } else {
        Write-Host ("  ok    {0}" -f $_.Name)
    }
}

Write-Host "`n== lint =="
Set-PSRepository PSGallery -InstallationPolicy Trusted -ErrorAction SilentlyContinue
# Pinned, and imported explicitly. Without the pin the import fails on this
# image, Invoke-ScriptAnalyzer returns nothing, and an empty result reads
# exactly like a clean one — which it did, once, and reported a false pass.
Install-Module PSScriptAnalyzer -RequiredVersion $env:ANALYZER -Scope CurrentUser -Force
Import-Module PSScriptAnalyzer
Write-Host ("  analyzer {0}" -f (Get-Module PSScriptAnalyzer).Version)

# Three rules are excluded, by name and with a reason each, because a checker
# nobody can get to green is a checker nobody runs. Printed rather than hidden,
# so dropping one is an edit rather than an argument.
#
#   PSAvoidUsingWriteHost - every script here is a console installer whose
#       output is the point, and Write-Output would lose the coloured Warn and
#       Die lines that make a failure legible.
#   PSAvoidOverwritingBuiltInCmdlets - fires on Write-Log, which these scripts
#       define as a local helper. The rule's own data claims a built-in of that
#       name; a script-scope function of two lines is not the hazard the rule
#       is aimed at, and renaming it across three shipping installers that
#       cannot be run here is a worse trade than the warning.
#   PSUseShouldProcessForStateChangingFunctions - fires on Stop-Installed and
#       Remove-Tree in install.ps1, which are internal helpers rather than
#       exported cmdlets. -WhatIf on a private function nobody can call is
#       ceremony.
#
# Everything else stays on, including the null-comparison rule that caught a
# real bug in install-gui.ps1 the first time this ran.
$excluded = @(
    "PSAvoidUsingWriteHost",
    "PSAvoidOverwritingBuiltInCmdlets",
    "PSUseShouldProcessForStateChangingFunctions"
)
Write-Host ("  excluded: {0}" -f ($excluded -join ", "))
$results = @(Invoke-ScriptAnalyzer -Path /repo/scripts -Severity Error,Warning `
    -ExcludeRule $excluded)
if ($results.Count -eq 0) {
    Write-Host "  clean"
} else {
    Write-Host ("  {0} finding(s)" -f $results.Count)
    $results | Sort-Object ScriptName, Line | ForEach-Object {
        Write-Host ("  {0,-7} {1}:{2}  {3}" -f $_.Severity, $_.ScriptName, $_.Line, $_.RuleName)
        Write-Host ("          {0}" -f $_.Message)
    }
    $script:failed = 1
}

if ($failed) { exit 1 }
Write-Host "`nPowerShell scripts parse and lint clean."
INNER

docker run --rm --pull=missing \
    -v "$REPO:/repo:ro" -v "$work/run.ps1:/run.ps1:ro" \
    -e "ANALYZER=$ANALYZER" \
    "$IMAGE" pwsh -NoProfile -File /run.ps1
