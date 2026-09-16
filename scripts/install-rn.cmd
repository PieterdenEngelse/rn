@echo off
rem  Install rn on Windows. Double-click this file.
rem
rem  Why a .cmd and not the .ps1 next to it: double-clicking a .ps1 opens it in
rem  Notepad. Windows will not execute a PowerShell script from Explorer, by
rem  design, and no amount of wanting changes that. A .cmd *is* executed on
rem  double-click, so this one exists to be the thing that gets clicked: it
rem  fetches scripts/install-gui.ps1 and runs it with -FromRelease, which
rem  downloads the published package, checks it against its published sha256,
rem  and installs it.
rem
rem  install-gui.ps1 rather than install.ps1: it is a face over install.ps1 and
rem  decides nothing, but it asks before installing, names each step while it
rem  runs, and offers to open rn at the end. This console window stays for the
rem  fetch and as the place errors can still land, deliberately -- a host with
rem  no Windows Forms falls back to installing right here, and hiding the
rem  window would hide that too.
rem
rem  What it needs on the machine: nothing. Not Rust, not Node, not git, not
rem  gh. Windows ships the PowerShell this uses.
rem
rem  Expect one prompt before any of it runs. A file downloaded from the
rem  internet carries a mark saying so, and Windows asks once whether you meant
rem  to run it. That prompt is the security boundary working, not a fault.
rem
rem  This file must stay CRLF and must never gain a BOM: cmd.exe would try to
rem  execute the BOM as part of the first line and fail with something
rem  unreadable. Its .ps1 neighbours need the opposite (see install.ps1).

setlocal
set "REPO=PieterdenEngelse/rn"
set "URL=https://raw.githubusercontent.com/%REPO%/main/scripts/install-gui.ps1"
set "PS1=%TEMP%\rn-install-gui.ps1"

echo.
echo   Installing rn from the latest release.
echo   Nothing else needs to be installed first.
echo.

echo   Fetching the installer. It will ask before it installs anything.
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ProgressPreference='SilentlyContinue'; try { Invoke-WebRequest -UseBasicParsing -Uri '%URL%' -OutFile '%PS1%' } catch { Write-Host $_.Exception.Message; exit 1 }"
if errorlevel 1 goto nodownload

rem  -File rather than -Command: the installer's own parameters stay parameters
rem  instead of being re-parsed out of a string. Bypass applies to this one
rem  invocation only and changes nothing about the machine's policy.
powershell -NoProfile -ExecutionPolicy Bypass -File "%PS1%" -FromRelease
if errorlevel 1 goto failed

del "%PS1%" >nul 2>&1
echo.
echo   Finished. The installer window says what it did.
echo.
pause
exit /b 0

:nodownload
echo.
echo   Could not download the installer from:
echo     %URL%
echo   Check the network, then run this again.
echo.
pause
exit /b 1

:failed
echo.
echo   The install did not finish. The output above says where it stopped.
echo   The installer itself is still at:
echo     %PS1%
echo   so you can re-run it directly, for example:
echo     powershell -NoProfile -ExecutionPolicy Bypass -File "%PS1%" -FromRelease -NoAutostart -NoStart
echo.
pause
exit /b 1
