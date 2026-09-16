<#
.SYNOPSIS
    Build rn from this checkout and install it for the current user, on Windows.

.DESCRIPTION
    Three ways in, in the order you probably want them:

    -FromRelease   downloads the Windows package published on GitHub and
                   installs it. Needs nothing on this machine — no Rust, no
                   Node, no git, not even gh, because the repository is public
                   and the asset is fetched over plain HTTPS. This is the one
                   that makes the file useful on its own: save install.ps1
                   anywhere and run it.
    -SkipBuild     installs a package tree you already have, built here or
                   carried over from elsewhere.
    (neither)      builds rn from the checkout this script sits in, then
                   installs that. Needs Rust, Node and — unless -NoWeb — dx.

    The Windows package it installs is cross-built on Linux by
    `scripts/package.sh --target windows`: only the launcher and node.exe are
    platform-specific, and both can be produced there, so there is no Windows
    build machine anywhere in this path.

    This is still not the twin of scripts/install.sh. That one installs; this
    one can also build, because there is no package.ps1 to do it. When one
    exists, the build half moves into it and what is left here is the twin.

    The intermediate tree is left behind in dist\rn, exactly the layout
    docs/packaging.md §1 describes, so -SkipBuild can reinstall it without
    rebuilding — which is how you iterate on the install half in seconds
    instead of once per release build.

    What it does NOT touch: %USERPROFILE%\.config\rn, where the launcher keeps
    settings, state and credentials (launcher/src/layout.rs:356). The install
    directory is replaced whole on every upgrade, so nothing the user owns may
    live in it. The one file carried across is app\.env, which the launcher
    reads its ports from.

    NOT YET TESTED ON WINDOWS — written on Linux, like every other .ps1 here.
    Treat the first run as a review, and -NoWeb -NoAutostart -NoStart as the
    smallest first step.

    Saved as UTF-8 *with* a BOM, deliberately: Windows PowerShell 5.1 — what a
    stock Windows box runs — reads a BOM-less file as the system ANSI codepage,
    which turns every em dash and section sign in the output below into
    mojibake. Do not let an editor strip it — every .ps1 here carries one for
    the same reason, and a .sh file must never be given one, because a shebang
    has to be the literal first bytes of the file.

    PowerShell may refuse to run this at all if the repo came off the internet:
        Unblock-File .\scripts\*.ps1
        powershell -ExecutionPolicy Bypass -File .\scripts\install.ps1

.EXAMPLE
    .\install.ps1 -FromRelease                   # no toolchain needed at all
    .\install.ps1 -FromRelease v0.1.0            # a particular release
    .\scripts\install.ps1                        # build from this checkout
    .\scripts\install.ps1 -NoWeb                 # skip the wasm page build
    .\scripts\install.ps1 -SkipBuild             # install dist\rn as it stands
    .\scripts\install.ps1 -Prefix D:\apps\rn
    .\scripts\install.ps1 -Uninstall
#>
[CmdletBinding()]
param(
    # Where the built tree is assembled. Inspectable, reusable, and the thing
    # a future package.ps1 would produce on its own.
    [string] $Out,
    # Where it gets installed. Per-user and no admin prompt, the Windows
    # analogue of ~/.local/share/rn (docs/packaging.md §8).
    [string] $Prefix,
    [switch] $NoWeb,
    [switch] $NoAutostart,
    [switch] $NoStart,
    # Passed to install-node.ps1. Off by default because gpg is rarely on a
    # Windows box; on for anything you would hand to someone else.
    [switch] $RequireSig,
    [switch] $SkipBuild,
    # Download a published package instead of building one. Takes an optional
    # tag; without one it resolves to the latest release.
    [switch] $FromRelease,
    [string] $Tag,
    [string] $Repo = "PieterdenEngelse/rn",
    [switch] $Uninstall
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Out)    { $Out    = Join-Path $RepoRoot "dist\rn" }
if (-not $Prefix) { $Prefix = Join-Path $env:LOCALAPPDATA "Programs\rn" }

$TaskName   = "rn"
$ConfigDir  = Join-Path $env:USERPROFILE ".config\rn"
$StartMenu  = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$Shortcut   = Join-Path $StartMenu "rn.url"

function Write-Step($m) { Write-Host "`n==> $m" }
function Write-Log($m)  { Write-Host "  $m" }
function Write-Warn($m) { Write-Host "  ! $m" -ForegroundColor Yellow }
function Die($m)        { Write-Host "`nERROR: $m" -ForegroundColor Red; exit 1 }

# Three things this script needs from the host, checked by name rather than
# discovered by failing. install.sh has always named sha256sum, tar, gzip and
# curl up front; the equivalents here are cmdlets, so they were invisible and
# went unnamed, which is worse rather than better — a missing cmdlet reports
# itself as "not recognized as the name of a cmdlet", naming the cmdlet and no
# remedy at all.

# 1. The PowerShell version. Expand-Archive arrived in 5.0 and Get-FileHash in
#    4.0, and Windows 10 and 11 ship 5.1 in the box, so this only ever fires on
#    something older or deliberately stripped. It cannot help if the script
#    fails to *parse* on a much older host — nothing in the file can — but a
#    version that parses and then dies on a missing cmdlet is the likelier
#    case, and this catches that one first and says what to do.
if ($PSVersionTable.PSVersion.Major -lt 5) {
    Die ("this needs Windows PowerShell 5.0 or newer, and this is $($PSVersionTable.PSVersion).`n" +
         "       Windows 10 and 11 ship 5.1 already. On an older machine, install`n" +
         "       PowerShell 7 from https://aka.ms/powershell and run this with pwsh.")
}

# 2. TLS 1.2, before anything reaches GitHub. Windows PowerShell inherits the
#    .NET default, which on an older or unpatched machine is still TLS 1.0/1.1
#    — and GitHub refuses those. The failure is the famously unhelpful "The
#    underlying connection was closed: An unexpected error occurred on a send",
#    which names neither TLS nor a remedy, and reads exactly like the network
#    being down. PowerShell 6+ negotiates properly and needs none of this;
#    -bor rather than assignment so nothing already enabled is switched off,
#    and in a try because the enum member is absent on very old .NET.
if ($PSVersionTable.PSVersion.Major -lt 6) {
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        Write-Host "  ! could not enable TLS 1.2; a download from GitHub may fail" -ForegroundColor Yellow
    }
}

# 3. Named, with what it is for and what to do instead — the shape install.sh
#    uses for sha256sum, tar and gzip.
function Assert-Cmdlet($Name, $Why, $Fix) {
    if (Get-Command $Name -ErrorAction SilentlyContinue) { return }
    Die "$Name is not available here, and $Why.`n       $Fix"
}

# Absolute, and normalised, without requiring the path to exist yet — the
# equivalent of `realpath -m`, which install.sh runs on the same two values.
function Resolve-Full($p) { [System.IO.Path]::GetFullPath([System.IO.Path]::Combine((Get-Location).Path, $p)) }
$Out    = Resolve-Full $Out
$Prefix = Resolve-Full $Prefix

# Native commands do not throw on failure however $ErrorActionPreference is
# set, so every one of them goes through here. A build that half-failed and
# carried on is the failure mode this exists to prevent.
function Invoke-Native {
    param([string] $What, [scriptblock] $Body)
    & $Body
    if ($LASTEXITCODE -ne 0) { Die "$What failed (exit $LASTEXITCODE)" }
}

function Test-OnPath($exe) { $null -ne (Get-Command $exe -ErrorAction SilentlyContinue) }

# Measure-Object over an empty tree returns a null Sum, and dividing null
# throws under StrictMode — a size report is not worth failing an install for.
function Get-TreeSizeMB($dir) {
    $sum = (Get-ChildItem $dir -Recurse -File -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
    if (-not $sum) { return 0 }
    [math]::Round($sum / 1MB, 1)
}

# ---------------------------------------------------------------------------
# Processes and locks
#
# Windows will not delete a directory holding a running executable, so an
# upgrade or an uninstall has to stop what it is replacing first. This is the
# one part of install.sh that does not carry over at all: on Linux the tree is
# renamed out from under a running process without complaint.
# ---------------------------------------------------------------------------

function Get-RunningUnder($dir) {
    Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.StartsWith($dir, [StringComparison]::OrdinalIgnoreCase) }
        catch { $false }   # .Path throws for processes we may not query
    }
}

# Stop by the task, never by `rn.exe --stop`: the pidfile is per user, not per
# install (launcher/src/pidfile.rs:20), so on a machine that also runs a
# development backend --stop names that one instead. Same reasoning as the
# comment above stop_service() in install.sh.
function Stop-Installed {
    $task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($task -and $task.State -eq "Running") {
        Write-Log "stopping scheduled task $TaskName"
        Stop-ScheduledTask -TaskName $TaskName
    }
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        $procs = @(Get-RunningUnder $Prefix)
        if ($procs.Count -eq 0) { return }
        Start-Sleep -Milliseconds 500
    }
    $left = @(Get-RunningUnder $Prefix | ForEach-Object { "$($_.ProcessName) (pid $($_.Id))" })
    if ($left.Count -gt 0) {
        # Stopping the task should take the whole tree with it; if the backend
        # outlived it, say which process and how to end it rather than deleting
        # files out from under something that is still writing to them.
        Die ("still running out of ${Prefix}: $($left -join ', ').`n" +
             "       End it (taskkill /PID <pid> /T), then re-run.")
    }
}

# A directory that was in use a moment ago often needs a second attempt: the
# handle outlives the process by a little.
function Remove-Tree($dir) {
    if (-not (Test-Path $dir)) { return }
    foreach ($attempt in 1..4) {
        try { Remove-Item $dir -Recurse -Force; return }
        catch { if ($attempt -eq 4) { throw }; Start-Sleep -Seconds 1 }
    }
}

# BACKEND_PORT out of app\.env, read the way the launcher reads it; 3010
# otherwise. Same default and same precedence as be/src/config.ts.
function Get-ApiPort {
    $envFile = Join-Path $Prefix "app\.env"
    if (Test-Path $envFile) {
        $hit = Select-String -Path $envFile -Pattern '^\s*BACKEND_PORT\s*=\s*["'']?(\d+)' |
               Select-Object -Last 1
        if ($hit) { return [int] $hit.Matches[0].Groups[1].Value }
    }
    return 3010
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------

if ($Uninstall) {
    Write-Step "Uninstalling $Prefix"
    Stop-Installed
    if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
        Write-Log "removed scheduled task $TaskName"
    }
    if (Test-Path $Shortcut) { Remove-Item $Shortcut -Force; Write-Log "removed the Start Menu entry" }
    if (Test-Path (Join-Path $Prefix "rn.exe")) {
        Remove-Tree $Prefix
        Write-Log "removed $Prefix"
    } else {
        Write-Warn "$Prefix holds no rn.exe — left alone"
    }
    Write-Log "kept $ConfigDir (settings, state, credentials); delete it by hand to forget everything"
    exit 0
}

# ---------------------------------------------------------------------------
# -FromRelease — the package comes from GitHub instead of from a build here
#
# The Linux twin shells out to gh for this. Windows does not have gh by
# default and the repository is public, so plain HTTPS is both simpler and one
# fewer thing to install: the release endpoints below need no token. The
# checksum published beside the zip is verified before anything is unpacked,
# which is the whole reason this is not a two-line Invoke-WebRequest.
# ---------------------------------------------------------------------------

$Asset = "rn-windows-x64.zip"

if ($FromRelease) {
    if ($SkipBuild) { Die "-FromRelease and -SkipBuild both say where the package comes from; pick one" }
    Assert-Cmdlet "Invoke-WebRequest" "it downloads the release" `
        "Use -SkipBuild with a package copied here by hand instead."
    Assert-Cmdlet "Get-FileHash" "it checks the download against its published sha256" `
        "Refusing to install something unverified; use -SkipBuild with a package you trust."
    Assert-Cmdlet "Expand-Archive" "it unpacks the package" `
        "Expand-Archive arrived in PowerShell 5.0. Unzip it by hand and use -SkipBuild -Out <dir>."
    $base = if ($Tag) { "https://github.com/$Repo/releases/download/$Tag" }
            else      { "https://github.com/$Repo/releases/latest/download" }
    $dl = Join-Path ([System.IO.Path]::GetTempPath()) ("rn-rel-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $dl | Out-Null

    Write-Step "Downloading $(if ($Tag) { $Tag } else { 'the latest release' }) from $Repo"
    $zip = Join-Path $dl $Asset
    $sum = "$zip.sha256"
    # The progress bar makes Invoke-WebRequest an order of magnitude slower on
    # a file this size — the same note install-node.ps1 carries.
    $old = $ProgressPreference; $ProgressPreference = "SilentlyContinue"
    try {
        foreach ($pair in @(@($Asset, $zip), @("$Asset.sha256", $sum))) {
            Write-Log "fetch: $($pair[0])"
            try { Invoke-WebRequest -Uri "$base/$($pair[0])" -OutFile $pair[1] -UseBasicParsing }
            catch { Die "download failed: $base/$($pair[0])`n       $($_.Exception.Message)" }
        }
    } finally { $ProgressPreference = $old }

    Write-Step "Verifying"
    # The .sha256 is written by sha256sum on Linux: "<hash>  <filename>", and
    # its hash is lowercase where Get-FileHash returns uppercase. -ne is
    # case-insensitive, so they compare equal — verified, both ways round.
    # Do not "tighten" this to -cne: that is case-sensitive and would reject
    # every honest download.
    $expected = ((Get-Content $sum -Raw) -split '\s+')[0]
    $actual = (Get-FileHash -Path $zip -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        Remove-Item $zip -Force      # never leave a bad artifact where it may be reused
        Die ("sha256 mismatch: the download does not match its published checksum, so it was deleted.`n" +
             "       expected $expected`n       got      $actual")
    }
    Write-Log "sha256 OK"

    Write-Step "Unpacking"
    $unpacked = Join-Path $dl "x"
    Expand-Archive -Path $zip -DestinationPath $unpacked -Force
    # The zip holds one directory; the package is inside it.
    $found = Get-ChildItem $unpacked -Recurse -Filter "rn.exe" | Select-Object -First 1
    if (-not $found) { Die "the archive holds no rn.exe" }
    $Out = $found.Directory.FullName
    Write-Log "package: $Out"
    $SkipBuild = $true               # there is nothing left to build
}

# ---------------------------------------------------------------------------
# Build — the half that package.sh does on Linux
# ---------------------------------------------------------------------------

if (-not $SkipBuild) {
    Write-Step "Checking the toolchain"
    if (-not (Test-OnPath "cargo")) { Die "cargo is not on PATH; it builds the launcher" }
    if (-not (Test-OnPath "npm"))   { Die "npm is not on PATH; it builds app\node_modules" }
    if (-not $NoWeb -and -not (Test-OnPath "dx")) {
        Die "dx is not on PATH; it builds the page. Install dioxus-cli, or pass -NoWeb."
    }
    Write-Log "cargo: $((cargo --version) -join '')"
    Write-Log "npm:   $((npm --version) -join '')"

    # dev-target.sh picks a per-worktree CARGO_TARGET_DIR for the tmux grid on
    # the Linux box; there is no grid here, so honour an ambient override and
    # otherwise use the workspace default.
    $TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RepoRoot "target" }

    foreach ($bad in @([System.IO.Path]::GetPathRoot($Out), $env:USERPROFILE, $RepoRoot,
                       (Join-Path $RepoRoot "be"), (Join-Path $RepoRoot "fe"))) {
        if ($Out -eq (Resolve-Full $bad)) { Die "refusing to build into $Out, which gets deleted first" }
    }

    Write-Step "Output: $Out"
    Remove-Tree $Out
    New-Item -ItemType Directory -Force -Path (Join-Path $Out "app") | Out-Null

    Write-Step "Launcher (release build)"
    Push-Location $RepoRoot
    try { Invoke-Native "cargo build" { cargo build --release -p rn } } finally { Pop-Location }
    $builtExe = Join-Path $TargetDir "release\rn.exe"
    if (-not (Test-Path $builtExe)) { Die "cargo built no $builtExe" }
    Copy-Item $builtExe (Join-Path $Out "rn.exe")
    Write-Log "rn.exe  $([math]::Round((Get-Item $builtExe).Length / 1MB, 1)) MB"

    Write-Step "Runtime"
    $nodeArgs = @("-Dest", (Join-Path $Out "runtime"))
    if ($RequireSig) { $nodeArgs += "-RequireSig" } else {
        Write-Warn "the runtime is checked by checksum only; pass -RequireSig to demand a signature too"
    }
    & (Join-Path $PSScriptRoot "install-node.ps1") @nodeArgs
    if ($LASTEXITCODE -ne 0) { Die "runtime install failed" }

    Write-Step "Backend (app\)"
    Copy-Item (Join-Path $RepoRoot "be\src") (Join-Path $Out "app\src") -Recurse
    # runtime-params.json is the registry the launcher reads; .env.example is
    # read beside .env by the settings page to say which keys exist.
    foreach ($f in @("package.json", "package-lock.json", "runtime-params.json", ".env.example")) {
        Copy-Item (Join-Path $RepoRoot "be\$f") (Join-Path $Out "app\$f")
    }
    # docs/packaging.md §3.4: npm ci at build time, ship the tree, so the
    # installed app never runs npm. --ignore-scripts because an install script
    # is arbitrary code and none of these dependencies needs one.
    #
    # package.sh prefixes PATH with the bundled runtime so npm runs under the
    # Node that ships. That does not transfer: npm.cmd prefers the node.exe
    # beside itself, and the bundled runtime carries no npm. Harmless here —
    # with --ignore-scripts and no native addons, npm only unpacks tarballs.
    Push-Location (Join-Path $Out "app")
    try {
        Invoke-Native "npm ci" { npm ci --omit=dev --ignore-scripts --no-audit --no-fund --loglevel=error }
    } finally { Pop-Location }
    Write-Log "app\node_modules  $(Get-TreeSizeMB (Join-Path $Out 'app\node_modules')) MB"

    if ($NoWeb) {
        Write-Warn "-NoWeb: no page in this build. The installed backend runs, and says at boot that it has none."
    } else {
        Write-Step "Page (release wasm build of fe; memory-hungry from cold)"
        Push-Location (Join-Path $RepoRoot "fe")
        try {
            if (-not (Test-Path "node_modules")) {
                Write-Log "fe\node_modules missing — npm ci first (tailwindcss builds the stylesheet)"
                Invoke-Native "npm ci" { npm ci --no-audit --no-fund --loglevel=error }
            }
            Invoke-Native "css:build" { npm run --silent css:build }

            # RN_API_BASE is read at compile time and cargo does not rebuild
            # when an environment variable changes, so the one file that reads
            # it is touched to force it (fe/src/api/client.rs:28).
            $client = Join-Path $RepoRoot "fe\src\api\client.rs"
            (Get-Item $client).LastWriteTime = Get-Date

            # dx never prunes its release output: every bundle's hashed js and
            # wasm stay in the public directory and the next bundle copies all
            # of them out. Starting empty keeps the build to what index.html
            # actually loads.
            Remove-Tree (Join-Path $TargetDir "dx\fe\release\web\public")

            $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("rn-web-" + [guid]::NewGuid().ToString("N"))
            $dxLog = "$tmp.log"
            try {
                # The Windows trap in this whole script. `$env:RN_API_BASE = ""`
                # DELETES the variable rather than setting it empty — that is
                # documented .NET behaviour — and option_env! then answers None,
                # which bakes the development address http://127.0.0.1:3010 into
                # the page. So the child process gets an environment built by
                # hand, where an empty value survives. The grep further down is
                # the backstop if this ever stops working.
                $psi = [System.Diagnostics.ProcessStartInfo]::new()
                $psi.FileName = (Get-Command dx).Source
                $dxArgs = @("bundle", "--web", "--release", "--debug-symbols=false", "--out-dir", $tmp)
                # ArgumentList is .NET Core only, so Windows PowerShell 5.1 —
                # what a stock Windows box runs — has to be handed a quoted
                # string instead. $tmp is the only argument that can contain a
                # space, and quoting it is enough.
                if ($psi.PSObject.Properties.Name -contains "ArgumentList") {
                    foreach ($a in $dxArgs) { $psi.ArgumentList.Add($a) }
                } else {
                    $psi.Arguments = ($dxArgs | ForEach-Object {
                        if ($_ -match '\s') { '"' + $_ + '"' } else { $_ }
                    }) -join " "
                }
                $psi.WorkingDirectory = (Join-Path $RepoRoot "fe")
                $psi.UseShellExecute = $false
                $psi.RedirectStandardOutput = $true
                $psi.RedirectStandardError = $true
                $psi.Environment["RN_API_BASE"] = ""
                $proc = [System.Diagnostics.Process]::Start($psi)
                # stderr async, stdout sync: reading both synchronously
                # deadlocks as soon as one pipe fills, and dx is verbose.
                $errTask = $proc.StandardError.ReadToEndAsync()
                $stdout  = $proc.StandardOutput.ReadToEnd()
                $proc.WaitForExit()
                $combined = $stdout + $errTask.Result
                $combined | Set-Content $dxLog
                Write-Host $combined
                if ($proc.ExitCode -ne 0) { Die "dx bundle failed (exit $($proc.ExitCode))" }

                # dx carries on after a wasm-opt abort and still exits 0, which
                # is why its output is read rather than trusted.
                if ($combined -match 'wasm-opt failed') {
                    Write-Warn "wasm-opt failed, so the page ships unoptimised: larger, still working."
                }

                $index = Get-ChildItem $tmp -Recurse -Filter "index.html" | Select-Object -First 1
                if (-not $index) { Die "dx bundle wrote no index.html under $tmp" }
                Copy-Item $index.Directory.FullName (Join-Path $Out "app\web") -Recurse

                $wasm = @(Get-ChildItem (Join-Path $Out "app\web") -Recurse -Filter "*.wasm")
                if ($wasm.Count -ne 1) {
                    Die "app\web holds $($wasm.Count) wasm files, expected 1: leftovers from an earlier bundle"
                }
                # The failure this catches is silent otherwise: a page that
                # loads from the install and then calls a development backend
                # on :3010 for every request.
                $bytes = [System.IO.File]::ReadAllBytes($wasm[0].FullName)
                $text  = [System.Text.Encoding]::ASCII.GetString($bytes)
                if ($text.Contains("http://127.0.0.1:3010")) {
                    Die ("the page was compiled with the development API address.`n" +
                         "       RN_API_BASE did not reach the compiler as an empty value" +
                         " — see the comment above this step.")
                }
                Write-Log "app\web  $(Get-TreeSizeMB (Join-Path $Out 'app\web')) MB"
            } finally {
                Remove-Tree $tmp
                if (Test-Path $dxLog) { Remove-Item $dxLog -Force }
            }
        } finally { Pop-Location }
    }

    $commit = try { (git -C $RepoRoot describe --always --dirty 2>$null) } catch { "unknown" }
    @(
        "rn $commit, built $((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mmZ')) on windows"
        "node $(Get-Content (Join-Path $Out 'runtime\VERSION') -Raw)".Trim()
        "page $(if ($NoWeb) { 'not included (-NoWeb)' } else { 'included' })"
        "runtime signature $(if ($RequireSig) { 'verified' } else { 'NOT verified' })"
        "built from a checkout by scripts\install.ps1, not by a packager"
    ) | Set-Content (Join-Path $Out "BUILD")
}

# ---------------------------------------------------------------------------
# Install — the half that install.sh does on Linux
# ---------------------------------------------------------------------------

Write-Step "Checking the build"
foreach ($f in @("rn.exe", "runtime\bin\node.exe", "app\src\server.ts",
                 "app\runtime-params.json", "app\node_modules")) {
    if (-not (Test-Path (Join-Path $Out $f))) {
        Die "not a complete package: $Out\$f is missing (build one, or use -FromRelease)"
    }
}
if ($Out -eq $Prefix) { Die "this build already is the install at $Prefix" }
Write-Log "build:   $Out"
Write-Log "node:    $((Get-Content (Join-Path $Out 'runtime\VERSION') -Raw).Trim())"
if (Test-Path (Join-Path $Out "BUILD")) { Write-Log "stamp:   $((Get-Content (Join-Path $Out 'BUILD'))[0])" }
if (Test-Path (Join-Path $Out "app\web\index.html")) {
    Write-Log "page:    app\web, served by the backend on the API's own port"
} else {
    Write-Warn "this build has no app\web: the backend will run, but has no page of its own to serve"
}

# The install directory is deleted and replaced, so be sure it is ours.
foreach ($bad in @([System.IO.Path]::GetPathRoot($Prefix), $env:USERPROFILE, $env:LOCALAPPDATA, $RepoRoot)) {
    if ($Prefix -eq (Resolve-Full $bad)) { Die "refusing to use $Prefix as the install directory" }
}
if ((Test-Path $Prefix) -and -not (Test-Path (Join-Path $Prefix "rn.exe")) -and
    @(Get-ChildItem $Prefix -Force).Count -gt 0) {
    Die "$Prefix exists and is not an rn install; not replacing it"
}

Write-Step "Installing to $Prefix"
Stop-Installed
$staging = "$Prefix.new"
Remove-Tree $staging
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Prefix) | Out-Null
Copy-Item $Out $staging -Recurse
$userEnv = Join-Path $Prefix "app\.env"
if (Test-Path $userEnv) {
    Copy-Item $userEnv (Join-Path $staging "app\.env")
    Write-Log "kept app\.env from the previous install"
}
# Remove-then-rename rather than copy over: a half-copied tree is never the
# live one. The Linux twin swaps the other way round — old aside, new in, old
# deleted — which Windows cannot do while anything holds a handle, so the
# stop above is load-bearing here in a way it is not there.
Remove-Tree $Prefix
Move-Item $staging $Prefix
Write-Log "installed, $(Get-TreeSizeMB $Prefix) MB"

$port = Get-ApiPort
$url  = "http://127.0.0.1:$port/"

if (-not $NoAutostart) {
    Write-Step "Autostart and Start Menu entry"
    # A logon scheduled task is the nearest thing to the systemd user unit the
    # Linux install writes: it survives a logout, restarts on failure, and
    # needs no administrator. A Windows service would need admin *and* a
    # launcher that speaks the service control protocol, which rn.exe does not.
    # The twin of install.sh's have_systemd check, and it dies the same way
    # rather than skipping quietly: an install that silently never starts is
    # the failure people spend an evening on.
    Assert-Cmdlet "Register-ScheduledTask" "it is what starts rn when you log on" `
        "Rerun with -NoAutostart and start $Prefix\rn.exe yourself."
    $action = New-ScheduledTaskAction -Execute (Join-Path $Prefix "rn.exe") `
                                      -WorkingDirectory (Join-Path $Prefix "app")
    # DOMAIN\user, not a bare name: the bare form is accepted in some places
    # and silently matches nothing in others.
    $me = if ($env:USERDOMAIN) { "$env:USERDOMAIN\$env:USERNAME" } else { $env:USERNAME }
    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $me
    $principal = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive -RunLevel Limited
    # ExecutionTimeLimit 0 means "no limit": the default kills a task after
    # three days, which for a supervised backend is a mystery outage.
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
                                             -DontStopOnIdleEnd -RestartCount 3 `
                                             -RestartInterval (New-TimeSpan -Minutes 1) `
                                             -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
                           -Principal $principal -Settings $settings -Force `
                           -Description "rn (installed: the launcher and the Node process it supervises)" | Out-Null
    Write-Log "registered the logon task $TaskName"
    Write-Warn "rn.exe is a console program, so the task shows a console window at logon."
    Write-Warn "That is the honest behaviour of this build; hiding it needs a windowless launcher."

    # A .url, not a .lnk: the entry opens the page, and the backend behind it
    # is the task's job — the same split as the .desktop file on Linux, which
    # runs xdg-open rather than the launcher.
    New-Item -ItemType Directory -Force -Path $StartMenu | Out-Null
    Set-Content -Path $Shortcut -Value "[InternetShortcut]`r`nURL=$url"
    Write-Log "the Start Menu entry opens $url"
}

if (-not $NoStart) {
    Write-Step "Starting"
    $holder = Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue |
              Select-Object -First 1
    if ($holder) {
        $who = try { (Get-Process -Id $holder.OwningProcess).ProcessName } catch { "unknown" }
        Write-Warn "port $port is already taken by $who (pid $($holder.OwningProcess)), so rn was not started."
        Write-Warn "stop that first, or set BACKEND_PORT in $Prefix\app\.env, then start the task."
    } elseif ($NoAutostart) {
        # No task to start it with, and starting rn.exe from here would tie it
        # to this console: it would die with the window. Say so instead.
        Write-Log "no autostart was registered — run $Prefix\rn.exe yourself"
    } else {
        Start-ScheduledTask -TaskName $TaskName
        $up = $false
        foreach ($attempt in 1..30) {
            try {
                Invoke-WebRequest -Uri "${url}api/health" -UseBasicParsing -TimeoutSec 2 | Out-Null
                $up = $true; break
            } catch { Start-Sleep -Milliseconds 500 }
        }
        if ($up) {
            Write-Log "running: $url"
        } else {
            Write-Warn "started, but ${url}api/health is not answering yet."
            Write-Warn "Check it with: (Get-ScheduledTask $TaskName).State, then $Prefix\rn.exe --status"
        }
    }
}

Write-Step "Done"
Write-Log "launcher:  $Prefix\rn.exe  (--status, --print-env)"
Write-Log "settings:  $ConfigDir  (never touched by install or uninstall)"
Write-Log "update:    .\install.ps1 -FromRelease   rebuild: .\scripts\install.ps1"
Write-Log "remove:    .\scripts\install.ps1 -Uninstall"
