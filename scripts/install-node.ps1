<#
.SYNOPSIS
    Install a private Node runtime on Windows.

.DESCRIPTION
    The Windows counterpart of scripts/install-node.sh, deliberately mirroring
    it step for step: same version source (be\.nvmrc), same mandatory checksum,
    same optional signature, same layout on disk, same idempotency.

    If you change one script, change the other in the same commit.

    NOT YET TESTED ON WINDOWS — written on Linux alongside the shell version.
    Treat the first run as a review.

.EXAMPLE
    .\scripts\install-node.ps1
    .\scripts\install-node.ps1 -Dest dist\runtime -RequireSig
#>
[CmdletBinding()]
param(
    [string] $Dest,
    [string] $Version,
    [string] $Platform = "win-x64",
    [switch] $RequireSig,
    [switch] $Force
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Dest)    { $Dest = Join-Path $RepoRoot "be\runtime" }
$VersionFile = Join-Path $RepoRoot "be\.nvmrc"

function Write-Step($m) { Write-Host "`n==> $m" }
function Write-Log($m)  { Write-Host "  $m" }
function Die($m)        { Write-Error "`nERROR: $m"; exit 1 }

# --- version: one source of truth -------------------------------------------
if (-not $Version) {
    if (-not (Test-Path $VersionFile)) { Die "no -Version given and $VersionFile is missing" }
    $Version = (Get-Content $VersionFile -Raw).Trim()
}
if (-not $Version.StartsWith("v")) { $Version = "v$Version" }

# --- architecture ------------------------------------------------------------
if (-not $PSBoundParameters.ContainsKey("Platform")) {
    $Platform = switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { "win-x64" }
        "ARM64" { "win-arm64" }
        default { Die "unsupported architecture $($env:PROCESSOR_ARCHITECTURE)" }
    }
}

$Zip     = "node-$Version-$Platform.zip"
$BaseUrl = "https://nodejs.org/dist/$Version"
$Cache   = if ($env:RN_CACHE_DIR) { $env:RN_CACHE_DIR } else { Join-Path $env:LOCALAPPDATA "rn\node" }

Write-Step "Node $Version for $Platform -> $Dest"

# --- already installed? ------------------------------------------------------
$NodeExe = Join-Path $Dest "bin\node.exe"
if (-not $Force -and (Test-Path $NodeExe)) {
    $have = (& $NodeExe --version 2>$null)
    if ($have -eq $Version) {
        Write-Log "already installed ($have) — nothing to do. Use -Force to reinstall."
        exit 0
    }
    Write-Log "replacing $have with $Version"
}

New-Item -ItemType Directory -Force -Path $Cache | Out-Null

# --- download (cached) -------------------------------------------------------
Write-Step "Downloading"
foreach ($f in @($Zip, "SHASUMS256.txt", "SHASUMS256.txt.asc")) {
    $target = Join-Path $Cache $f
    if ($f -eq $Zip -and (Test-Path $target) -and (Get-Item $target).Length -gt 0) {
        Write-Log "cached: $f"
    } else {
        Write-Log "fetch:  $f"
        # ProgressPreference stays SilentlyContinue: the progress bar makes
        # Invoke-WebRequest an order of magnitude slower on large files.
        $old = $ProgressPreference; $ProgressPreference = "SilentlyContinue"
        try   { Invoke-WebRequest -Uri "$BaseUrl/$f" -OutFile $target -UseBasicParsing }
        catch { Die "download failed: $BaseUrl/$f" }
        finally { $ProgressPreference = $old }
    }
}

# --- verify: checksum mandatory, signature opt-in ----------------------------
Write-Step "Verifying"
$zipPath  = Join-Path $Cache $Zip
$expected = (Select-String -Path (Join-Path $Cache "SHASUMS256.txt") -Pattern "\s$([regex]::Escape($Zip))$" `
             | Select-Object -First 1).Line -split '\s+' | Select-Object -First 1
if (-not $expected) { Die "$Zip is not listed in SHASUMS256.txt" }

$actual = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
if ($actual -ne $expected.ToLower()) {
    Remove-Item $zipPath -Force        # never leave a bad artifact in the cache
    Die "SHA-256 mismatch for $Zip. Removed it; re-run to download again."
}
Write-Log "sha256 OK"

$gpg = Get-Command gpg -ErrorAction SilentlyContinue
$sigOk = $false
if ($gpg) {
    & gpg --verify (Join-Path $Cache "SHASUMS256.txt.asc") (Join-Path $Cache "SHASUMS256.txt") 2>$null
    $sigOk = ($LASTEXITCODE -eq 0)
}
if ($sigOk) {
    Write-Log "gpg signature OK"
} elseif ($RequireSig) {
    Die "signature verification failed or Node release keys are not in your keyring.
       Import them (see the nodejs/node README), then re-run."
} else {
    Write-Log "gpg signature NOT verified. Fine for development;"
    Write-Log "use -RequireSig for anything you ship."
}

# --- install -----------------------------------------------------------------
Write-Step "Installing"
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("rn-" + [guid]::NewGuid().ToString("N"))
try {
    Expand-Archive -Path $zipPath -DestinationPath $work -Force
    $src = Join-Path $work "node-$Version-$Platform"

    if (Test-Path $Dest) { Remove-Item $Dest -Recurse -Force }
    New-Item -ItemType Directory -Force -Path (Join-Path $Dest "bin") | Out-Null

    # The Windows zip is laid out flat: node.exe sits at the root, not in bin/.
    # We normalise to bin\node.exe so the launcher path is identical on every
    # platform.
    Copy-Item (Join-Path $src "node.exe") $NodeExe
    Copy-Item (Join-Path $src "LICENSE")  (Join-Path $Dest "LICENSE")
    Set-Content -Path (Join-Path $Dest "VERSION") -Value $Version

    # No strip on Windows: the official node.exe ships without separate debug
    # symbols, so there is nothing to remove.
} finally {
    if (Test-Path $work) { Remove-Item $work -Recurse -Force }
}

# --- prove it runs -----------------------------------------------------------
$got = (& $NodeExe --version)
if ($got -ne $Version) { Die "installed binary reports $got, expected $Version" }

Write-Step "Done"
Write-Log "$NodeExe  ($got)"
