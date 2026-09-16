<#
.SYNOPSIS
    The graphical front door to scripts\install.ps1, for Windows.

.DESCRIPTION
    A face over install.ps1 that decides nothing. Every argument is handed
    straight through, install.ps1 still does the work, still writes the same
    text, and is still the file to read before running either.

        .\install-gui.ps1 -FromRelease     download a published release, install it
        .\install-gui.ps1 -SkipBuild       install a package tree already built
        .\install-gui.ps1                  build from this checkout, then install

    Three windows, matching scripts\install-gui.sh one for one: a confirmation
    naming where the files go and what is left alone, a progress window naming
    the step that is running, and a result window offering to open rn or to
    show the log.

    WHY THIS EXISTS, AND THE ARGUMENT IT OVERRULES. install-rn.cmd is already
    clickable, and a .cmd *is* a console window, so Windows had a way to see
    what an install was doing that Linux did not. That was the reason given for
    building install-gui.sh and stopping there. The counter-argument is
    simpler: a console full of scrolling text is what the tool happens to emit,
    not what someone installing an app should have to read, and "it already
    reports somewhere" is a different claim from "a person can tell what
    happened". Both platforms now get the same three windows.

    NOT TESTED ON WINDOWS. Stronger than the warning install.ps1 carries,
    because this file cannot be tested here even in principle: WinForms is a
    Windows-only assembly and there is no PowerShell on the machine this was
    written on at all. What was actually done: every .ps1 here is parsed by
    PowerShell 7.4 in a container, and this one is written to fail *visibly*
    rather than silently when an assumption is wrong. Treat the first run as a
    review, and -NoWeb -NoAutostart -NoStart as the smallest first step.

    The console is the fallback, not the failure case. If WinForms will not
    load — PowerShell Core without the Windows Desktop runtime, Server Core,
    an unusual host — this runs install.ps1 in the console instead and says so.
    That path uses nothing but what install.ps1 already needs, so the graphical
    half can be entirely wrong and the install still works.

    Saved as UTF-8 with a BOM, like every other .ps1 here: Windows PowerShell
    5.1 reads a BOM-less file as the system ANSI codepage and turns every em
    dash below into mojibake.

.EXAMPLE
    .\install-gui.ps1 -FromRelease
#>
[CmdletBinding()]
param(
    # Not a copy of install.ps1's parameter block, deliberately. A second copy
    # would drift the first time install.ps1 grew a switch, and this file has
    # no opinion about any of them: they are collected and forwarded verbatim.
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $Rest
)

$ErrorActionPreference = "Stop"

$Repo    = "PieterdenEngelse/rn"
$RawUrl  = "https://raw.githubusercontent.com/$Repo/main/scripts/install.ps1"
$Title   = "Install rn"
$LogPath = Join-Path $env:LOCALAPPDATA "rn-install.log"

$Forward = @()
if ($Rest) { $Forward = @($Rest) }

# ---------------------------------------------------------------- the installer

# Prefer the install.ps1 sitting next to this file: in a checkout and in a
# built package they are neighbours, and fetching a copy of a file that is
# already here would install something other than what was read.
$Installer = $null
$Temp      = $null
$Here      = if ($PSScriptRoot) { $PSScriptRoot } else { $null }
if ($Here -and (Test-Path (Join-Path $Here "install.ps1"))) {
    $Installer = Join-Path $Here "install.ps1"
}

function Test-LocalPackage {
    if (-not $Here) { return $false }
    Test-Path (Join-Path $Here "rn.exe")
}

function Test-WantsRelease {
    foreach ($a in $Forward) { if ($a -ieq "-FromRelease") { return $true } }
    return $false
}

# Double-clicking on a machine that has never seen rn means there is no package
# to install and no toolchain to build one. install.ps1 would try to build,
# which is the right default for a checkout and a dead end for a launcher, so
# supply the flag. An explicit -FromRelease is left alone, and a real package
# beside us still wins.
if (-not (Test-WantsRelease) -and -not (Test-LocalPackage)) {
    $Forward += "-FromRelease"
}

# install.ps1 takes -Prefix and this file forwards it, so the confirmation has
# to name the directory that will actually be used rather than the default it
# assumes. Mirrored, not interpreted: install.ps1 still decides.
$Prefix = Join-Path $env:LOCALAPPDATA "Programs\rn"
for ($i = 0; $i -lt $Forward.Count; $i++) {
    if ($Forward[$i] -ieq "-Prefix" -and ($i + 1) -lt $Forward.Count) { $Prefix = $Forward[$i + 1] }
}

# ------------------------------------------------------------------- the windows

# One try, and the result decides everything below. Add-Type throws on a host
# with no WinForms rather than returning anything, so this is the only place
# that has to care.
$Gui = $true
try {
    Add-Type -AssemblyName System.Windows.Forms -ErrorAction Stop
    Add-Type -AssemblyName System.Drawing -ErrorAction Stop
    [System.Windows.Forms.Application]::EnableVisualStyles()
} catch {
    $Gui = $false
}

function Show-Ask($Text, $Icon) {
    # MessageBox rather than a hand-built form with better button labels. The
    # labels would read better; this cannot be tested here, and a dialog that
    # is certain to appear beats one that reads well in the source. Yes/No with
    # a clear question is not ambiguous.
    [System.Windows.Forms.MessageBox]::Show(
        $Text, $Title,
        [System.Windows.Forms.MessageBoxButtons]::YesNo,
        $Icon) -eq [System.Windows.Forms.DialogResult]::Yes
}

function Show-Info($Text, $Icon) {
    [System.Windows.Forms.MessageBox]::Show(
        $Text, $Title,
        [System.Windows.Forms.MessageBoxButtons]::OK,
        $Icon) | Out-Null
}

# The progress window. install.ps1 runs as its own process writing to a log,
# and a timer reads that log — the same shape install-gui.sh uses, and for the
# same reason: nothing the window does can kill the install. Closing it stops
# the watching.
#
# Redirected to files rather than to pipes this script drains itself. The pipe
# version needed Register-ObjectEvent to collect output, and those -Action
# blocks run in their own session state and cannot see a local $sb — the label
# would have stayed on "Starting..." for the whole install, on Windows, where
# it could not be reproduced here. A file has no such subtlety.
function Read-Shared($Path) {
    # Opened with FileShare.ReadWrite because the installer still has it open
    # for writing. Get-Content is not documented to share, and "file in use"
    # once a second would be an unreadable window rather than a stuck one.
    if (-not (Test-Path -LiteralPath $Path)) { return "" }
    try {
        $fs = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open,
                                     [System.IO.FileAccess]::Read,
                                     [System.IO.FileShare]::ReadWrite)
        try {
            $sr = New-Object System.IO.StreamReader($fs)
            return $sr.ReadToEnd()
        } finally { $fs.Dispose() }
    } catch { return "" }
}

function ConvertTo-CommandLineArg($Value) {
    if ($Value -match '[\s"]') { return '"' + ($Value -replace '"', '\"') + '"' }
    return $Value
}

function Invoke-WithProgress($Exe, $ArgList) {
    $form = New-Object System.Windows.Forms.Form
    $form.Text            = $Title
    $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
    $form.StartPosition   = [System.Windows.Forms.FormStartPosition]::CenterScreen
    $form.MaximizeBox     = $false
    $form.MinimizeBox     = $false
    $form.ClientSize      = New-Object System.Drawing.Size(460, 120)

    $label = New-Object System.Windows.Forms.Label
    $label.Text     = "Starting..."
    $label.AutoSize = $false
    $label.Location = New-Object System.Drawing.Point(16, 18)
    $label.Size     = New-Object System.Drawing.Size(428, 40)
    $form.Controls.Add($label)

    # Marquee, not a percentage. The total is genuinely unknown — a download of
    # unknown size, then a copy — and a bar that invents a number is lying
    # about the one thing it exists to say.
    $bar = New-Object System.Windows.Forms.ProgressBar
    $bar.Style    = [System.Windows.Forms.ProgressBarStyle]::Marquee
    $bar.Location = New-Object System.Drawing.Point(16, 68)
    $bar.Size     = New-Object System.Drawing.Size(428, 22)
    $form.Controls.Add($bar)

    # Script scope, because the tick handler runs later and reaching a
    # function's locals from an event handler is the kind of thing that works
    # until it does not. These are read by the handler below.
    $script:GuiProc  = $null
    $script:GuiForm  = $form
    $script:GuiLabel = $label
    $script:ErrPath  = "$LogPath.err"

    "" | Set-Content -LiteralPath $LogPath -Encoding UTF8
    "" | Set-Content -LiteralPath $script:ErrPath -Encoding UTF8

    # Start-Process rather than a hand-built ProcessStartInfo: it redirects
    # straight to files, so nothing here has to drain a pipe. An undrained pipe
    # fills its buffer and blocks the child forever, which looks exactly like
    # an install that hung partway through. stdout and stderr must be different
    # files — Start-Process rejects the same path for both.
    $script:GuiProc = Start-Process -FilePath $Exe -ArgumentList $ArgList -PassThru `
        -NoNewWindow -RedirectStandardOutput $LogPath -RedirectStandardError $script:ErrPath

    $timer = New-Object System.Windows.Forms.Timer
    $timer.Interval = 400
    $timer.Add_Tick({
        # The step name comes from install.ps1's own "==> " lines rather than
        # from a list kept here, which would drift the first time a step was
        # added.
        $text = Read-Shared $LogPath
        if ($text) {
            $steps = [regex]::Matches($text, '(?m)^==>\s*(.+)$')
            if ($steps.Count -gt 0) {
                $script:GuiLabel.Text = $steps[$steps.Count - 1].Groups[1].Value.Trim()
            }
        }
        if ($script:GuiProc.HasExited) {
            $this.Stop()
            $script:GuiForm.Close()
        }
    })
    $timer.Start()
    [void]$form.ShowDialog()
    $timer.Stop()
    $timer.Dispose()

    # The window may have been closed by hand while the install ran on. It is
    # still ours to wait for: reporting a result before the work finished would
    # be worse than the wait.
    $script:GuiProc.WaitForExit()
    return $script:GuiProc.ExitCode
}

# --------------------------------------------------------------------- go

$PsExe = (Get-Process -Id $PID).Path
if (-not $PsExe) { $PsExe = "powershell.exe" }

function Invoke-Console {
    if (-not $Installer) {
        Write-Host "downloading the installer from $RawUrl"
        $script:Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("rn-install-" + [guid]::NewGuid().ToString("N") + ".ps1")
        Invoke-WebRequest -UseBasicParsing -Uri $RawUrl -OutFile $script:Temp
        $script:Installer = $script:Temp
    }
    & $PsExe -NoProfile -ExecutionPolicy Bypass -File $Installer @Args
    exit $LASTEXITCODE
}

if (-not $Gui) {
    Write-Host "No Windows Forms on this host, so the install runs here in the console instead."
    Write-Host "Nothing is lost: the windows are a face over the same script."
    Invoke-Console
}

# The confirmation says where the files go and what is left alone, because "it
# installed something somewhere" is the complaint an installer earns by not
# saying. The wording changes on an upgrade: replacing an install someone is
# already running is a different act from adding one.
if (Test-Path (Join-Path $Prefix "rn.exe")) {
    $what = "rn is already installed in`r`n$Prefix`r`n`r`nUpgrading replaces that folder whole and restarts it."
} else {
    $what = "rn will be installed for this user only, in`r`n$Prefix`r`n`r`nNo administrator prompt, nothing in Program Files, nothing added to PATH."
}
if (Test-WantsRelease) {
    $where = "The package comes from the latest GitHub release of $Repo, and is checked against the sha256 published beside it before anything is unpacked. It carries its own Node, so nothing else has to be installed first."
} else {
    $where = "rn is built from this checkout and the result installed. That needs Rust and Node on this machine."
}
$settings = Join-Path $env:USERPROFILE ".config\rn"
if (-not (Show-Ask "$what`r`n`r`n$where`r`n`r`nYour settings, state and credentials in $settings are never touched, by an install or by an uninstall.`r`n`r`nInstall rn now?" ([System.Windows.Forms.MessageBoxIcon]::Question))) {
    exit 0
}

if (-not $Installer) {
    try {
        $Temp = Join-Path ([System.IO.Path]::GetTempPath()) ("rn-install-" + [guid]::NewGuid().ToString("N") + ".ps1")
        Invoke-WebRequest -UseBasicParsing -Uri $RawUrl -OutFile $Temp
        $Installer = $Temp
    } catch {
        Show-Info "Could not download the installer from:`r`n$RawUrl`r`n`r`n$($_.Exception.Message)`r`n`r`nCheck the network, then try again." ([System.Windows.Forms.MessageBoxIcon]::Error)
        exit 1
    }
}

# Quoted only where a space or quote makes it necessary. Wrapping a switch
# in quotes is usually harmless and occasionally is not, and there is no way
# to find out which from here.
$quoted = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", (ConvertTo-CommandLineArg $Installer))
foreach ($a in $Forward) { $quoted += (ConvertTo-CommandLineArg $a) }
$rc = Invoke-WithProgress $PsExe ($quoted -join " ")

if ($Temp -and (Test-Path $Temp)) { Remove-Item $Temp -Force -ErrorAction SilentlyContinue }

# ------------------------------------------------------------------ result

# Both streams. install.ps1's Die writes through Write-Host so it lands on
# stdout, but a PowerShell exception on the way to it does not, and a result
# window quoting the wrong stream would name nothing.
$log = ""
if (Test-Path -LiteralPath $LogPath) { $log = (Get-Content -LiteralPath $LogPath -Raw) }
if ($script:ErrPath -and (Test-Path -LiteralPath $script:ErrPath)) {
    $err = (Get-Content -LiteralPath $script:ErrPath -Raw)
    if ($err -and $err.Trim()) { $log = $log + "`r`n" + $err }
    Remove-Item -LiteralPath $script:ErrPath -Force -ErrorAction SilentlyContinue
}
$log | Set-Content -LiteralPath $LogPath -Encoding UTF8

if ($rc -ne 0) {
    # The last non-empty lines carry install.ps1's own Die message, which names
    # what failed. The log has the rest.
    $tail = ($log -split "`r?`n" | Where-Object { $_.Trim() } | Select-Object -Last 3) -join "`r`n"
    if (Show-Ask "The install did not finish.`r`n`r`n$tail`r`n`r`nThe full log is at $LogPath" ([System.Windows.Forms.MessageBoxIcon]::Error)) {
        Start-Process notepad.exe $LogPath
    }
    exit 1
}

# install.ps1 prints the URL only once it has answered a health check, so its
# presence in the log is the difference between "installed" and "running".
$url = $null
$m = [regex]::Match($log, 'http://127\.0\.0\.1:\d+/')
if ($m.Success) { $url = $m.Value }
$warnings = ($log -split "`r?`n" | Where-Object { $_ -match '^\s+!\s' } | ForEach-Object { $_.Trim() -replace '^!\s*', '- ' }) -join "`r`n"

$done = "rn is installed in`r`n$Prefix`r`n`r`nIt starts when you log on. The Start Menu entry opens it."
if ($warnings) { $done += "`r`n`r`nWorth reading:`r`n$warnings" }

if ($url) {
    if (Show-Ask "$done`r`n`r`nIt is running at $url`r`n`r`nOpen rn now?" ([System.Windows.Forms.MessageBoxIcon]::Information)) {
        Start-Process $url
    }
} else {
    Show-Info $done ([System.Windows.Forms.MessageBoxIcon]::Information)
}
exit 0
