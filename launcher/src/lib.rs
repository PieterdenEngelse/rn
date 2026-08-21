//! rn launcher internals, exposed as a library so the enforcement tests can
//! reach them. The binary is a thin shell over this.

pub mod layout;
pub mod node_command;
pub mod pidfile;
pub mod settings;
pub mod shutdown;

/// The child asks for a restart by exiting with this code. Distinct from
/// anything Node uses itself (1-13 for its own failures, 128+n for signals).
pub const EXIT_RESTART: i32 = 75;
