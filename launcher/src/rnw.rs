//! rnw — the same launcher as rn, without a console window.
//!
//! What the Windows installer starts, after installing and at sign-in. It is
//! main.rs compiled a second time for the Windows GUI subsystem, so it can
//! never behave differently from rn.exe except in having no window: its output,
//! and the Node child's, go to the log file `console::log_path()` names.
//!
//! Run rn.exe, not this, from a terminal: a GUI-subsystem program does not hold
//! the prompt, and prints nothing there. launcher/src/console.rs has the why.
//!
//! On every other platform the attribute is ignored and this is just rn again.

#![windows_subsystem = "windows"]

#[path = "main.rs"]
mod app;

fn main() {
    rn::console::log_to_file();
    app::main();
}
