<#
.SYNOPSIS
    Wrap the Windows package in an MSI installer with a setup wizard.

.DESCRIPTION
    Takes the tree `scripts/package.sh --target windows` builds (dist\rn-win)
    and builds rn-windows-x64.msi from scripts\msi\rn.wxs with WiX v5. What
    the installer shows and does is written at the top of that file.

    Runs on Windows: WiX's MSI back end is Windows-only. The release workflow
    runs it on its Windows runner, after the Linux runner has cross-built the
    tree. It used to be package-msi.sh with wixl on Linux, which cannot build
    dialogs — so that MSI explained nothing, and this one exists to.

    Needs the WiX v5 .NET tool and its UI extension:
        dotnet tool install --global wix --version 5.0.2
        wix extension add --global WixToolset.UI.wixext/5.0.2

    Saved as UTF-8 with a BOM, like every .ps1 here, for Windows PowerShell 5.1.

.EXAMPLE
    .\scripts\package-msi.ps1
    .\scripts\package-msi.ps1 -From dist\rn-win -Out dist\rn-windows-x64.msi
#>
[CmdletBinding()]
param(
    [string] $From,
    [string] $Out
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $From) { $From = Join-Path $RepoRoot "dist\rn-win" }
if (-not $Out)  { $Out  = Join-Path $RepoRoot "dist\rn-windows-x64.msi" }
$From = [IO.Path]::GetFullPath($From)
$Out  = [IO.Path]::GetFullPath($Out)

function Die($m) { Write-Host "`nERROR: $m" -ForegroundColor Red; exit 1 }

if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    Die "wix is not on PATH: dotnet tool install --global wix --version 5.0.2"
}
foreach ($f in "rn.exe", "rnw.exe", "runtime\bin\node.exe", "app\src\server.ts", "BUILD") {
    if (-not (Test-Path (Join-Path $From $f))) {
        Die "$From is not a Windows package: $f is missing (scripts/package.sh --target windows)"
    }
}

# One version, from the crate that is the app. An MSI version is three numbers;
# anything else installs and then fails to upgrade, silently.
$cargo = Get-Content (Join-Path $RepoRoot "launcher\Cargo.toml") -Raw
if ($cargo -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"') {
    Die "launcher/Cargo.toml has no x.y.z version, which an MSI version has to be"
}
$version = $Matches[1]

Write-Host "`n==> Staging the package"
$work  = Join-Path ([IO.Path]::GetTempPath()) ("rn-msi-" + [guid]::NewGuid().ToString("N"))
$stage = Join-Path $work "rn"
New-Item -ItemType Directory -Force -Path $work | Out-Null
Copy-Item $From $stage -Recurse
try {
    # The PowerShell installers stay out: run against an MSI install they would
    # delete files Windows Installer believes it owns.
    Remove-Item (Join-Path $stage "install.ps1"), (Join-Path $stage "install-gui.ps1") -ErrorAction SilentlyContinue
    # The Start Menu entry opens this. 3010 is the default port (be/src/config.ts).
    [IO.File]::WriteAllText((Join-Path $stage "rn.url"), "[InternetShortcut]`r`nURL=http://127.0.0.1:3010/`r`n")
    $staged = @(Get-ChildItem $stage -Recurse -File).Count
    Write-Host "  files: $staged"

    Write-Host "`n==> Building $(Split-Path $Out -Leaf) (rn $version)"
    New-Item -ItemType Directory -Force -Path (Split-Path $Out) | Out-Null
    $msiDir = Join-Path $PSScriptRoot "msi"
    & wix build (Join-Path $msiDir "rn.wxs") `
        -ext WixToolset.UI.wixext `
        -culture en-US -loc (Join-Path $msiDir "rn.en-us.wxl") `
        -arch x64 `
        -d "Version=$version" -d "SourceDir=$stage" `
        -o $Out
    if ($LASTEXITCODE -ne 0) { Die "wix build failed (exit $LASTEXITCODE)" }

    # Checked, not trusted: the File table against what was staged.
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $db = $installer.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $installer, @($Out, 0))
    $view = $db.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $db, @("SELECT ``File`` FROM ``File``"))
    $view.GetType().InvokeMember("Execute", "InvokeMethod", $null, $view, $null) | Out-Null
    $inMsi = 0
    while ($view.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $view, $null)) { $inMsi++ }
    $view.GetType().InvokeMember("Close", "InvokeMethod", $null, $view, $null) | Out-Null
    if ($inMsi -ne $staged) { Die "the MSI holds $inMsi files but $staged were staged" }
    Write-Host "  $(Split-Path $Out -Leaf)  $([math]::Round((Get-Item $Out).Length / 1MB, 1)) MB, $inMsi files"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
