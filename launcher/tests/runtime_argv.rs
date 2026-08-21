//! Each runtime takes a different command line. These lock the dialects down,
//! because the failure mode is silent: spawn the right binary with the wrong
//! argv and it dies on an unknown flag with nothing useful in the log.

use rn::layout::{bind_address, net_allowlist, runtime_argv, RuntimeKind};
use std::path::Path;

fn strings(kind: RuntimeKind, env_file: &Path) -> Vec<String> {
    runtime_argv(kind, env_file, Path::new("/app/src/server.ts"), &["127.0.0.1:3010".to_string()])
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

/// A path that cannot exist, so the "no .env" branch is exercised.
fn missing() -> &'static Path {
    Path::new("/nonexistent-dir-for-tests/.env")
}

#[test]
fn node_gets_the_if_exists_form() {
    let argv = strings(RuntimeKind::Node, missing());
    assert_eq!(
        argv,
        vec![
            "--env-file-if-exists=/nonexistent-dir-for-tests/.env".to_string(),
            "/app/src/server.ts".to_string(),
        ]
    );
}

#[test]
fn bun_does_not_get_a_node_flag() {
    let argv = strings(RuntimeKind::Bun, missing());
    assert_eq!(argv, vec!["run".to_string(), "/app/src/server.ts".to_string()]);
    // --env-file-if-exists is Node-only; passing it to bun is an unknown flag.
    assert!(!argv.iter().any(|a| a.contains("env-file")));
}

#[test]
fn deno_names_its_permissions_and_runs_the_script_last() {
    let argv = strings(RuntimeKind::Deno, missing());
    assert_eq!(argv.first().map(String::as_str), Some("run"));
    for grant in ["--allow-env", "--allow-read", "--allow-write", "--allow-sys"] {
        assert!(argv.iter().any(|a| a == grant), "missing {grant}");
    }
    // Scoped, never blanket: a bare --allow-net gives back the guarantee that
    // is the entire reason to run under Deno.
    assert!(argv.iter().any(|a| a == "--allow-net=127.0.0.1:3010"));
    assert!(!argv.iter().any(|a| a == "--allow-net"), "blanket network grant");
    // Deno parses options before the script path, so the script must be last.
    assert_eq!(argv.last().map(String::as_str), Some("/app/src/server.ts"));
}

#[test]
fn deno_omits_env_file_when_there_is_none() {
    let argv = strings(RuntimeKind::Deno, missing());
    // Deno's --env-file has no "if exists" form and errors on a missing file.
    assert!(!argv.iter().any(|a| a.starts_with("--env-file")));
}

#[test]
fn deno_passes_env_file_when_it_exists() {
    // Any file that certainly exists works — only is_file() is consulted.
    let argv = strings(RuntimeKind::Deno, Path::new("/etc/hostname"));
    assert!(argv.iter().any(|a| a == "--env-file=/etc/hostname"));
}

#[test]
fn the_bind_address_is_always_granted() {
    // Without it the server cannot listen, so it can never be omitted however
    // the allowlist setting is written.
    assert_eq!(net_allowlist("127.0.0.1", 3010, ""), vec!["127.0.0.1:3010"]);
    assert_eq!(net_allowlist("0.0.0.0", 8080, "   "), vec!["0.0.0.0:8080"]);
}

#[test]
fn extra_hosts_are_split_trimmed_and_deduped() {
    assert_eq!(
        net_allowlist("127.0.0.1", 3010, "api.example.com, 10.0.0.5:5432 ,,127.0.0.1:3010"),
        vec!["127.0.0.1:3010", "api.example.com", "10.0.0.5:5432"]
    );
}

#[test]
fn bind_address_falls_back_to_the_config_ts_defaults() {
    // Must match be/src/config.ts, or Deno denies the app its own socket.
    let (host, port) = bind_address(missing());
    assert_eq!((host.as_str(), port), ("127.0.0.1", 3010));
}

#[test]
fn bind_address_reads_the_env_file() {
    let dir = std::env::temp_dir().join("rn-bind-test");
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join(".env");
    std::fs::write(&f, "# comment\nBACKEND_HOST=\"0.0.0.0\"\nBACKEND_PORT=9999\n").unwrap();
    let (host, port) = bind_address(&f);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!((host.as_str(), port), ("0.0.0.0", 9999));
}
