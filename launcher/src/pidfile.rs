//! Knowing whether rn is already running, and stopping it by name.
//!
//! Without this the only way to stop the launcher is to hunt for it with
//! `pkill -f`, whose pattern happily matches the shell you typed it in — a
//! mistake that is very easy to make and kills the wrong process. A pidfile
//! makes `rn --stop` exact.

use std::fs;
use std::path::PathBuf;

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
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

/// The pid recorded in the file, if it is still running. A stale file (the
/// process died without cleaning up) reads as "not running".
pub fn running_pid() -> Option<i32> {
    let text = fs::read_to_string(path()).ok()?;
    let pid: i32 = text.trim().parse().ok()?;
    if is_alive(pid) {
        Some(pid)
    } else {
        None
    }
}

pub fn write_own() -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    fs::write(&p, std::process::id().to_string())
        .map_err(|e| format!("cannot write {}: {e}", p.display()))
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
    let ok = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        remove();
        Ok(())
    } else {
        Err(format!("could not stop pid {pid}"))
    }
}
