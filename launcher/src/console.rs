//! Running without a console window, on Windows.
//!
//! rn.exe is a console program, so Windows gives it a window whenever nothing
//! else has one to share: installed and started at sign-in, that was a black
//! window full of JSON log lines, and closing it stopped rn. Nobody installing
//! an app expects that, and the window explained nothing.
//!
//! So there are two launchers, the way Python has python.exe and pythonw.exe:
//! rn.exe for a terminal (`--status`, `--stop`, watching it run), and rnw.exe,
//! the same code built for the Windows GUI subsystem, which gets no window at
//! all. src/rnw.rs is that second binary; this module is what it needs.
//!
//! A windowless process still has to put its output somewhere, and so do the
//! processes it starts:
//!
//! - [`log_to_file`] points the launcher's stdout and stderr at a log file.
//!   The Node child inherits those handles, so its log lands in the same file.
//! - [`hide_window`] marks a child to be started without a console. Without
//!   it, Windows gives every console program a GUIless parent starts a new
//!   window of its own — Node, and also the tasklist and taskkill the pidfile
//!   runs, which would flash a window at every start.
//!
//! None of this does anything anywhere but Windows.

use std::path::PathBuf;

/// Where rnw.exe writes its log, and Node's: beside the pidfile, in rn's
/// per-user state directory, which survives an upgrade or uninstall.
pub fn log_path() -> PathBuf {
    crate::pidfile::path().with_file_name("rn.log")
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetConsoleWindow() -> *mut c_void;
        pub fn SetStdHandle(std_handle: u32, handle: *mut c_void) -> i32;
    }
}

/// Does this process have a console window of its own to share with children?
/// Always true off Windows, where the question does not arise.
pub fn has_console() -> bool {
    #[cfg(windows)]
    {
        // SAFETY: no arguments; returns a window handle or null.
        !unsafe { win::GetConsoleWindow() }.is_null()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Start `cmd` without a console window when this process has none: a child
/// of rnw.exe must not pop up a window, while a child of rn.exe in a terminal
/// keeps sharing that terminal, exactly as before.
pub fn hide_window(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if !has_console() {
            cmd.creation_flags(win::CREATE_NO_WINDOW);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// Send this process's stdout and stderr, and so its children's, to
/// [`log_path`]. The previous run's log is kept as rn.log.1, so the run that
/// just ended badly is still there to read after a restart.
///
/// Best effort: a launcher that cannot open its log still runs, silently,
/// rather than refusing to start over a log file.
pub fn log_to_file() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;

        let path = log_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::rename(&path, path.with_file_name("rn.log.1"));
        let Ok(file) = std::fs::File::create(&path) else {
            return;
        };
        // One handle for both streams, so lines from the launcher and from Node
        // stay in the order they were written.
        let handle = file.as_raw_handle();
        // SAFETY: a valid, open file handle; it must outlive every later write
        // to stdout or stderr, which is why the File is leaked below rather
        // than dropped.
        unsafe {
            win::SetStdHandle(win::STD_OUTPUT_HANDLE, handle.cast());
            win::SetStdHandle(win::STD_ERROR_HANDLE, handle.cast());
        }
        std::mem::forget(file);
    }
}
