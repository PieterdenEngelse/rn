//! Knowing whether rn is already running, and stopping it by name.
//!
//! Without this the only way to stop the launcher is to hunt for it with
//! `pkill -f`, whose pattern happily matches the shell you typed it in — a
//! mistake that is very easy to make and kills the wrong process. A pidfile
//! makes `rn --stop` exact.

use std::fs;
use std::path::{Path, PathBuf};

/// Where the record lives, unless a caller names a file itself.
///
/// `RN_PID_FILE` is an operator override — a way to run a second copy against
/// its own state. It is **not** a test seam, and must not be used as one: it is
/// process-global, cargo runs a binary's tests in parallel threads, and a test
/// that loses the race falls through to this default and reads *and writes* the
/// user's real pidfile. That is not hypothetical — two tests here did exactly
/// that, clobbering a live launcher's record in about a third of runs. Tests
/// name their file explicitly with [`running_at`] and [`write_own_at`].
pub fn path() -> PathBuf {
    if let Ok(explicit) = std::env::var("RN_PID_FILE") {
        return PathBuf::from(explicit);
    }
    let base = std::env::var("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|_| {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".local").join("state")
    });
    base.join("rn").join("rn.pid")
}

/// Is a process with this pid alive? Signal 0 checks without delivering.
#[cfg(unix)]
pub fn is_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(windows)]
pub fn is_alive(pid: i32) -> bool {
    // Best effort: ask the task list. Windows has no signal-0 equivalent here
    // without pulling in the Win32 API.
    //
    // clippy.toml bans Command::new so that every *Node* spawn goes through
    // NodeCommand with a sealed environment. This is not a Node spawn: it runs
    // a Windows tool, reads its output and exits. NodeCommand would be the
    // wrong thing here — it seals an environment for a child that is ours.
    #[allow(clippy::disallowed_methods)]
    let mut cmd = std::process::Command::new("tasklist");
    // rnw.exe runs this at every start; without the flag it flashes a window.
    crate::console::hide_window(&mut cmd);
    cmd.args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// What the pidfile records.
///
/// More than a pid, because a pid alone cannot answer the question that
/// actually matters when you find rn running: *who started this, and are they
/// still there?* A launcher started by a tool session that has since ended
/// keeps supervising the backend indefinitely, invisible to whoever is now at
/// the keyboard — that happened here, for 21 hours, and `--status` said only
/// "running (pid 40776)".
#[derive(Debug, Clone)]
pub struct Record {
    pub pid: i32,
    /// Epoch milliseconds. Uptime is the first clue that a launcher is older
    /// than the session you are in.
    pub started_at: u128,
    /// The parent at the moment rn started. Compared against the live parent
    /// later: once the original parent exits, the kernel reparents us and this
    /// no longer matches, which is what "orphaned" means here.
    pub parent_pid: i32,
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// The live parent of a running process, or None where it cannot be read.
#[cfg(unix)]
pub fn live_parent_of(pid: i32) -> Option<i32> {
    // Field 4 of /proc/<pid>/stat is ppid. The comm field can contain spaces
    // and parentheses, so split after the final ')' rather than on whitespace.
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(not(unix))]
pub fn live_parent_of(_pid: i32) -> Option<i32> {
    None
}

/// Whether the process that started this launcher is gone.
///
/// None where it cannot be determined, which is not the same as "no" and is
/// reported differently — a launcher we cannot vouch for should not be
/// described as healthy.
pub fn is_orphaned(rec: &Record) -> Option<bool> {
    // A record written before provenance existed has no parent to compare
    // against. Unknown, not orphaned — reporting a healthy launcher as orphaned
    // on the strength of a missing field would make the warning worthless
    // within one upgrade.
    if rec.parent_pid == 0 {
        return None;
    }
    let live = live_parent_of(rec.pid)?;
    Some(live != rec.parent_pid)
}

fn parse(text: &str) -> Option<Record> {
    let text = text.trim();
    // Older files held a bare pid and nothing else. Read them rather than
    // treating the launcher as stopped, which would let a second copy start.
    if let Ok(pid) = text.parse::<i32>() {
        return Some(Record { pid, started_at: 0, parent_pid: 0 });
    }
    let field = |name: &str| -> Option<&str> {
        text.split(&format!("\"{name}\":"))
            .nth(1)?
            .trim_start()
            .split(|c: char| c == ',' || c == '}')
            .next()
            .map(str::trim)
    };
    Some(Record {
        pid: field("pid")?.parse().ok()?,
        started_at: field("startedAt").and_then(|v| v.parse().ok()).unwrap_or(0),
        parent_pid: field("parentPid").and_then(|v| v.parse().ok()).unwrap_or(0),
    })
}

/// The record in `file`, if that process is still running.
///
/// Takes the path rather than reading it from the environment so a caller can
/// be certain which file it touched — see [`path`] for why that matters.
pub fn running_at(file: &Path) -> Option<Record> {
    let rec = parse(&fs::read_to_string(file).ok()?)?;
    if is_alive(rec.pid) {
        Some(rec)
    } else {
        None
    }
}

/// The record in the file, if that process is still running. A stale file (the
/// process died without cleaning up) reads as "not running".
pub fn running() -> Option<Record> {
    running_at(&path())
}

/// Just the pid, for callers that do not care who started it.
pub fn running_pid() -> Option<i32> {
    running().map(|r| r.pid)
}

pub fn write_own() -> Result<(), String> {
    write_own_at(&path())
}

/// Write this process's record to `file`, creating its directory.
///
/// The explicit-path half of [`write_own`], for the same reason as
/// [`running_at`] — and more urgently, because getting the file wrong here
/// overwrites a record rather than merely misreading one.
pub fn write_own_at(file: &Path) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let pid = std::process::id();
    let parent = live_parent_of(pid as i32).unwrap_or(0);
    // Hand-written rather than pulling serde into the launcher, which is
    // deliberately tiny — it has to run before Node exists.
    let body = format!(
        "{{\"pid\":{pid},\"startedAt\":{},\"parentPid\":{parent}}}",
        now_ms()
    );
    fs::write(file, body).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

pub fn remove() {
    let _ = fs::remove_file(path());
}

/// Ask a running rn to stop, and wait for it to actually be gone.
#[cfg(unix)]
pub fn stop(pid: i32, wait: std::time::Duration) -> Result<(), String> {
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(format!("could not signal pid {pid}"));
    }
    let deadline = std::time::Instant::now() + wait;
    while std::time::Instant::now() < deadline {
        if !is_alive(pid) {
            remove();
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!("pid {pid} did not exit within {}s", wait.as_secs()))
}

#[cfg(windows)]
pub fn stop(pid: i32, _wait: std::time::Duration) -> Result<(), String> {
    // A Windows tool, not Node — see the note on is_alive above.
    //
    // Plain taskkill asks politely, by closing the process's windows, and
    // rnw.exe has none to close: it answers "can only be terminated forcefully"
    // and nothing stops. So /F is the fallback when asking did not work.
    let kill = |force: bool| {
        #[allow(clippy::disallowed_methods)]
        let mut cmd = std::process::Command::new("taskkill");
        crate::console::hide_window(&mut cmd);
        cmd.args(["/PID", &pid.to_string(), "/T"]);
        if force {
            cmd.arg("/F");
        }
        cmd.output().map(|o| o.status.success()).unwrap_or(false)
    };
    let ok = kill(false) || kill(true);
    if ok {
        remove();
        Ok(())
    } else {
        Err(format!("could not stop pid {pid}"))
    }
}
