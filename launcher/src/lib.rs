//! rn launcher internals, exposed as a library so the enforcement tests can
//! reach them. The binary is a thin shell over this.

pub mod console;
pub mod credentials;
pub mod layout;
pub mod node_command;
pub mod pidfile;
pub mod settings;
pub mod shutdown;

/// The child asks for a restart by exiting with this code. Distinct from
/// anything Node uses itself (1-13 for its own failures, 128+n for signals).
pub const EXIT_RESTART: i32 = 75;

/// The child reports a failure restarting cannot fix, and asks not to be
/// restarted. 78 is sysexits' EX_CONFIG, which is what this always is in
/// practice: a port already in use, or an address it may not bind.
///
/// Without it the supervisor treats every non-zero exit alike and retries five
/// times in quick succession. For a port that is occupied that is five
/// pointless restarts, half a second apart, ending in a crash-loop message
/// about frequency — which describes the supervisor's own behaviour rather than
/// the problem. The child already knows the difference, so it says so.
pub const EXIT_FATAL: i32 = 78;
