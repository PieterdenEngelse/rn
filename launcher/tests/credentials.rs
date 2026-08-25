//! Credentials come from outside the install tree, and only from a file the
//! caller names.
//!
//! Every test here passes its own path. `read_from` takes one for that reason:
//! a process-global pointing at the real file is how the pidfile tests came to
//! read and overwrite a live launcher's record, and a secrets file is a worse
//! thing to reach by accident.

use rn::credentials::{self, PREFIX};
use rn::node_command::NodeCommand;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rn-creds-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("credentials")
}

fn write(name: &str, body: &str) -> PathBuf {
    let file = scratch(name);
    std::fs::write(&file, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    file
}

#[test]
fn a_missing_file_is_the_ordinary_case_and_not_an_error() {
    // Most installs have no credentials at all. Refusing to start over it
    // would leave someone with no UI in which to add one.
    let c = credentials::read_from(&scratch("absent").with_file_name("nothing-here"));
    assert!(c.vars.is_empty());
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
}

#[test]
fn prefixed_values_are_read_and_quotes_stripped() {
    let file = write(
        "read",
        "# a comment\n\nRN_SECRET_GITHUB_TOKEN=ghp_abc123\nRN_SECRET_HOOK=\"https://x/y\"\n",
    );
    let c = credentials::read_from(&file);
    assert_eq!(c.vars.get("RN_SECRET_GITHUB_TOKEN").map(String::as_str), Some("ghp_abc123"));
    // Quotes go the same way be/src/config.ts strips them from .env, so a
    // value pasted from one file behaves the same in the other.
    assert_eq!(c.vars.get("RN_SECRET_HOOK").map(String::as_str), Some("https://x/y"));
    assert!(c.warnings.is_empty(), "{:?}", c.warnings);
}

#[test]
fn a_value_containing_an_equals_sign_survives_whole() {
    // Base64 and query strings both contain them; splitting on the last would
    // silently truncate a working credential.
    let file = write("equals", "RN_SECRET_B64=YWJj=ZGVm=\n");
    let c = credentials::read_from(&file);
    assert_eq!(c.vars.get("RN_SECRET_B64").map(String::as_str), Some("YWJj=ZGVm="));
}

#[test]
fn an_unprefixed_key_is_ignored_out_loud() {
    // Silence would leave a credential that looks configured and is not —
    // the failure this whole feature exists to make visible.
    let file = write("unprefixed", "GITHUB_TOKEN=ghp_abc123\n");
    let c = credentials::read_from(&file);
    assert!(c.vars.is_empty(), "nothing without the prefix gets through");
    assert_eq!(c.warnings.len(), 1);
    assert!(c.warnings[0].contains(PREFIX), "{}", c.warnings[0]);
}

#[test]
fn an_empty_value_is_not_a_credential() {
    // Treating "" as set sends an empty Authorization header and comes back
    // 401 with nothing to explain it — the same rule as secrets.ts.
    let file = write("empty", "RN_SECRET_TOKEN=\n");
    let c = credentials::read_from(&file);
    assert!(c.vars.is_empty());
    assert_eq!(c.warnings.len(), 1);
}

#[test]
fn a_malformed_line_is_reported_without_quoting_it() {
    // A line that is not KEY=value might *be* the secret, so the warning names
    // the line number and nothing else.
    let file = write("malformed", "RN_SECRET_OK=fine\nghp_averyrealtoken\n");
    let c = credentials::read_from(&file);
    assert_eq!(c.vars.len(), 1);
    assert_eq!(c.warnings.len(), 1);
    assert!(!c.warnings[0].contains("ghp_averyrealtoken"), "{}", c.warnings[0]);
}

#[cfg(unix)]
#[test]
fn a_file_others_can_read_is_warned_about_but_still_read() {
    use std::os::unix::fs::PermissionsExt;
    let file = write("perms", "RN_SECRET_TOKEN=ghp_abc123\n");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    let c = credentials::read_from(&file);
    // Read anyway: refusing would break the app over a permission bit, and the
    // user needs the UI to fix anything at all.
    assert_eq!(c.vars.len(), 1);
    assert!(c.warnings.iter().any(|w| w.contains("chmod 600")), "{:?}", c.warnings);
}

#[test]
fn the_default_path_is_outside_the_install_tree() {
    // The whole point of the move. Inside the app directory it would be
    // replaced wholesale on upgrade, and the tokens would go with it.
    let p = credentials::path();
    assert!(p.ends_with(".config/rn/credentials"), "{}", p.display());
}

#[test]
fn a_credential_reaches_the_sealed_child() {
    // The seam that matters: env_clear() runs first, so a value only arrives
    // if the launcher put it there deliberately. This is the proof that
    // reading the file and sealing the environment are compatible rather than
    // merely believed to be.
    let node = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("launcher/ has a parent")
        .join("be/runtime/bin/node");
    if !node.is_file() {
        eprintln!("skipping: no runtime at {} — run scripts/install-node.sh", node.display());
        return;
    }

    let file = write("child", "RN_SECRET_REACHES=through-the-seal\n");
    let creds = credentials::read_from(&file);

    let install = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = NodeCommand::new(&node, &install)
        .envs(&creds.vars)
        .arg("-e")
        .arg("process.stdout.write(process.env.RN_SECRET_REACHES ?? 'missing')")
        .output()
        .expect("node runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "through-the-seal");
}
