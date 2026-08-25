//! Credentials, read from outside the install tree and handed to the child
//! explicitly.
//!
//! **Why the launcher does this at all.** The backend reads credentials from
//! its environment, one variable per credential (`RN_SECRET_<NAME>` — see
//! `be/src/secrets.ts`). The obvious place to put them was `be/.env`, and that
//! is where they were: inside the app directory, which is replaced wholesale on
//! upgrade. Tokens would have been destroyed by every upgrade, silently. It is
//! the same failure `settings.json`, the metric history and the run record were
//! all moved out of the install tree to avoid, and this is that move for the
//! one kind of value it would hurt most to lose.
//!
//! **Why it does not weaken the seal.** `NodeCommand` clears the environment
//! and allowlists what goes in, because a user's `NODE_OPTIONS` can stop the
//! app booting before its first line runs. Nothing here widens that: the values
//! are read from a file *we* name and passed through `NodeCommand::env`, which
//! is the door for values that come from us. No prefix allowlist, no
//! `allow_var`, no new trust in the ambient environment.
//!
//! **What this is not.** The file is plaintext. Anyone who can read the user's
//! home directory can read it — the same protection as an SSH private key with
//! no passphrase, which is a comparison people can calibrate against. That is
//! stated rather than dressed up, here and in `docs/sec.md`; an encrypted store
//! with a key from the OS keychain is the conventional next step and slots in
//! behind this module without any job changing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Only variables with this prefix are passed on.
///
/// The name-to-variable conversion (`githubToken` → `RN_SECRET_GITHUB_TOKEN`)
/// lives in `be/src/secrets.ts` and only there. The launcher matches a prefix
/// and copies a string, so there is no second implementation of that rule to
/// drift from the first.
pub const PREFIX: &str = "RN_SECRET_";

/// Where the file lives. Beside `settings.json`, and outside the install tree
/// for the same reason.
pub fn path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("rn").join("credentials")
}

/// What was found in the file, and anything the user should know about it.
///
/// Warnings rather than errors throughout. A permission bit or a stray line is
/// not a reason to refuse to start the app — that would leave someone with no
/// UI in which to fix it, which is the same argument the runtime selection
/// makes for never treating an unhonourable request as fatal.
pub struct Credentials {
    pub vars: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

/// Read `file`, keeping only the `RN_SECRET_` entries.
///
/// Takes the path rather than resolving it, so a test names its own file and
/// never touches the user's. That is not a style preference: two tests sharing
/// a process-global pointer at this file is exactly how the pidfile tests came
/// to read and overwrite a live launcher's record.
pub fn read_from(file: &Path) -> Credentials {
    let mut vars = BTreeMap::new();
    let mut warnings = Vec::new();

    let Ok(text) = std::fs::read_to_string(file) else {
        // Missing is the ordinary case: most installs have no credentials.
        return Credentials { vars, warnings };
    };

    if let Some(mode) = world_or_group_readable(file) {
        warnings.push(format!(
            "{} is readable by others (mode {mode:o}) — chmod 600 it",
            file.display(),
        ));
    }

    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            // Named but not quoted: a malformed line might *be* the secret.
            warnings.push(format!("{}:{} is not KEY=value", file.display(), n + 1));
            continue;
        };
        let key = key.trim();
        if !key.starts_with(PREFIX) {
            // Silence here would be a credential that looks configured and is
            // not — the failure this whole item exists to make visible.
            warnings.push(format!(
                "{}:{} ignored: {key} does not start with {PREFIX}",
                file.display(),
                n + 1,
            ));
            continue;
        }
        // Quotes are stripped the same way be/src/config.ts reads .env, so a
        // value pasted from one file behaves the same in the other. Nothing
        // else is trimmed from the middle: a credential may contain anything.
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        if value.is_empty() {
            warnings.push(format!("{}:{} {key} is empty", file.display(), n + 1));
            continue;
        }
        vars.insert(key.to_string(), value);
    }

    Credentials { vars, warnings }
}

/// The mode, when the file is readable by group or other. None otherwise.
#[cfg(unix)]
fn world_or_group_readable(file: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(file).ok()?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 { Some(mode) } else { None }
}

/// Windows has no mode bits worth reporting this way, and guessing at ACLs
/// would produce a warning nobody can act on.
#[cfg(not(unix))]
fn world_or_group_readable(_file: &Path) -> Option<u32> {
    None
}
