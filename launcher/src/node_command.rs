use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;
use std::process::{Child, Command};

/// A Node process with a sealed environment.
///
/// The inner `Command` is private and never handed back: there is no `Deref`,
/// no `inner()`, no public field. The only way to put a variable into the child
/// is [`NodeCommand::env`] or [`NodeCommand::allow_var`], both of which are
/// greppable and reviewable.
///
/// This exists because a user's `NODE_OPTIONS` can stop the app booting before
/// our first line of code runs — verified: `NODE_OPTIONS=--require=/nonexistent`
/// exits 1. `clippy.toml` bans `Command::new` everywhere else so this stays the
/// single spawn site.
pub struct NodeCommand(Command);

impl NodeCommand {
    /// Build a Node invocation with an environment constructed from nothing.
    pub fn new(node: &Path, install_dir: &Path) -> Self {
        #[allow(clippy::disallowed_methods)] // THE one sanctioned spawn site
        let mut c = Command::new(node);

        c.env_clear();

        // Minimal PATH for anything the app itself spawns. Not for finding Node
        // — that is always an absolute path.
        c.env("PATH", if cfg!(windows) { "C:\\Windows\\System32" } else { "/usr/bin:/bin" });

        if let Ok(home) = std::env::var("HOME") {
            c.env("HOME", home);
        }

        // Windows needs these or winsock and the crypto APIs fail in confusing
        // ways — a fully empty environment breaks TLS and DNS there.
        if cfg!(windows) {
            for key in ["SYSTEMROOT", "SystemRoot", "TEMP", "TMP", "USERPROFILE"] {
                if let Ok(v) = std::env::var(key) {
                    c.env(key, v);
                }
            }
        }

        // Marks the environment as ours. be/src/main.ts refuses to run in an
        // installed tree without it, and the Config page uses it to decide
        // whether a restart button can work.
        c.env("RN_ENV_SEALED", "1");
        c.env("RN_INSTALL_DIR", install_dir);
        // So the app can report which launcher is supervising it, and so a
        // status page can show something more useful than "supervised: true".
        c.env("RN_LAUNCHER_PID", std::process::id().to_string());

        Self(c)
    }

    /// Set a variable explicitly. The value comes from us, not from the ambient
    /// environment.
    pub fn env(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Self {
        self.0.env(key, value);
        self
    }

    pub fn envs(&mut self, vars: &BTreeMap<String, String>) -> &mut Self {
        for (k, v) in vars {
            self.0.env(k, v);
        }
        self
    }

    /// Pass exactly one variable through from the ambient environment, by name.
    /// The only door in — add to it one variable at a time, with a reason.
    pub fn allow_var(&mut self, key: &str) -> &mut Self {
        if let Ok(v) = std::env::var(key) {
            self.0.env(key, v);
        }
        self
    }

    pub fn arg(&mut self, arg: impl AsRef<OsStr>) -> &mut Self {
        self.0.arg(arg);
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Self {
        self.0.current_dir(dir);
        self
    }

    pub fn spawn(&mut self) -> std::io::Result<Child> {
        self.0.spawn()
    }

    pub fn output(&mut self) -> std::io::Result<std::process::Output> {
        self.0.output()
    }
}
