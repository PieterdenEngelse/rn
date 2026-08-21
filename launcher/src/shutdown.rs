//! Stopping cleanly.
//!
//! Without this the launcher dies on Ctrl-C and leaves Node running: an
//! orphaned process still holding port 3010, which the next start then fails
//! against with EADDRINUSE. So the signal has to be forwarded and the child
//! actually waited for.

use std::process::Child;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

/// PID of the running child, so the signal handler (which runs on its own
/// thread) can reach it. 0 means "no child right now".
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// Set once a stop has been requested. The supervisor checks it so an
/// intentional stop is never mistaken for a crash worth restarting.
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// How long Node gets to close its server before we stop being polite.
const GRACE: Duration = Duration::from_secs(5);

pub fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::SeqCst)
}

pub fn set_child(pid: Option<u32>) {
    CHILD_PID.store(pid.map(|p| p as i32).unwrap_or(0), Ordering::SeqCst);
}

/// Forward a termination signal to the child so it can exit on its own terms.
#[cfg(unix)]
fn signal_child(pid: i32) {
    // SIGTERM, not SIGKILL: server.ts handles it, closes the listener and
    // exits 0. SIGKILL would leave the port in TIME_WAIT and lose in-flight
    // work with no chance to finish.
    unsafe { libc::kill(pid, libc::SIGTERM) };
}

#[cfg(windows)]
fn signal_child(_pid: i32) {
    // A console Ctrl-C already reaches the whole process group, so the child
    // has been told. If it ignores that, the grace period below escalates.
}

/// Install the handler. Called once, before the first child is spawned.
pub fn install() -> Result<(), String> {
    ctrlc::set_handler(move || {
        if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
            // Second Ctrl-C: the user is insisting. Stop waiting.
            eprintln!("\nrn: second interrupt — exiting now");
            crate::pidfile::remove();
            std::process::exit(130);
        }
        eprintln!("\nrn: stopping (waiting up to {}s for Node)", GRACE.as_secs());
        let pid = CHILD_PID.load(Ordering::SeqCst);
        if pid > 0 {
            signal_child(pid);
        }
    })
    .map_err(|e| format!("cannot install signal handler: {e}"))
}

/// Wait for the child, staying responsive to a stop request.
///
/// `Child::wait()` blocks uninterruptibly, so the grace period could never be
/// enforced from here. Polling costs a 100ms tick and buys the ability to
/// escalate.
pub fn wait_for(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    let mut deadline: Option<Instant> = None;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }

        if is_shutting_down() {
            let d = *deadline.get_or_insert_with(|| Instant::now() + GRACE);
            if Instant::now() >= d {
                eprintln!("rn: Node did not exit within the grace period — killing it");
                let _ = child.kill();
                return child.wait();
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}
