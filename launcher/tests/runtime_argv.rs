//! Each runtime takes a different command line. These lock the dialects down,
//! because the failure mode is silent: spawn the right binary with the wrong
//! argv and it dies on an unknown flag with nothing useful in the log.

use rn::layout::{bind_address, hooks_port, net_allowlist, runtime_argv, tracker_port, RuntimeKind};
use std::path::Path;

fn strings(kind: RuntimeKind, env_file: &Path) -> Vec<String> {
    runtime_argv(kind, env_file, Path::new("/app/src/server.ts"), &["127.0.0.1:3010".to_string()], &[])
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
    assert_eq!(net_allowlist("127.0.0.1", 3010, &[3010], ""), vec!["127.0.0.1:3010"]);
    assert_eq!(net_allowlist("0.0.0.0", 8080, &[8080], "   "), vec!["0.0.0.0:8080"]);
}

#[test]
fn extra_hosts_are_split_trimmed_and_deduped() {
    assert_eq!(
        net_allowlist("127.0.0.1", 3010, &[3010], "api.example.com, 10.0.0.5:5432 ,,127.0.0.1:3010"),
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

// ---- pidfile provenance -------------------------------------------------
//
// These name their file with `running_at` / `write_own_at` rather than pointing
// `RN_PID_FILE` at it. That variable is process-global and cargo runs these
// tests in parallel threads, so one test's `remove_var` lands inside another's
// critical section; the loser falls through to the default path and reads —
// and, via `write_own`, *overwrites* — the user's real pidfile. It failed about
// a third of the time and clobbered a live launcher's record when it did.

/// The record is what makes an orphaned launcher visible. A bare pid cannot
/// answer "who started this, and are they still there?", which is the question
/// that matters when you find rn running and did not start it yourself.
#[test]
fn a_bare_pid_file_is_still_read() {
    // Written by a version before provenance existed. Reading it as "stopped"
    // would let a second copy start and crash-loop on the bound port.
    let dir = std::env::temp_dir().join(format!("rn-pidfile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bare.pid");
    std::fs::write(&file, format!("{}", std::process::id())).unwrap();

    let rec = rn::pidfile::running_at(&file).expect("a live bare pid must still read as running");
    assert_eq!(rec.pid, std::process::id() as i32);
    assert_eq!(rec.started_at, 0, "an old file has no start time to report");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_written_record_round_trips_with_its_provenance() {
    let dir = std::env::temp_dir().join(format!("rn-pidfile-rt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rn.pid");

    rn::pidfile::write_own_at(&file).unwrap();
    let rec = rn::pidfile::running_at(&file).expect("just written, so alive");

    assert_eq!(rec.pid, std::process::id() as i32);
    assert!(rec.started_at > 0, "a start time is recorded");
    // Not orphaned: this test process's parent is still whatever ran it.
    assert_eq!(rn::pidfile::is_orphaned(&rec), Some(false));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_record_whose_parent_has_changed_reads_as_orphaned() {
    // The 21-hour case, simulated: the recorded parent is not the live one.
    let rec = rn::pidfile::Record {
        pid: std::process::id() as i32,
        started_at: 1,
        parent_pid: -1, // cannot be a real ppid
    };
    assert_eq!(rn::pidfile::is_orphaned(&rec), Some(true));
}

#[test]
fn a_legacy_record_is_unknown_rather_than_orphaned() {
    // A pidfile written before provenance existed records no parent. Saying
    // "orphaned" on the strength of a missing field would fire on every
    // healthy launcher for one upgrade, and a warning that cries wolf once is
    // a warning nobody reads again.
    let rec = rn::pidfile::Record {
        pid: std::process::id() as i32,
        started_at: 0,
        parent_pid: 0,
    };
    assert_eq!(rn::pidfile::is_orphaned(&rec), None);
}

#[test]
fn the_hooks_port_is_granted_alongside_the_api() {
    // Under Deno an ungranted port is not a webhook feature that quietly does
    // nothing — the listener binds at startup, so it is a backend that will not
    // boot. Both sockets have to be in the grant.
    assert_eq!(
        net_allowlist("127.0.0.1", 3010, &[3011], ""),
        vec!["127.0.0.1:3010", "127.0.0.1:3011"]
    );
}

#[test]
fn every_listener_is_granted_and_none_twice() {
    // The tracker binds at startup like the hooks listener does, so it is the
    // same non-optional grant. The dedupe matters because the three ports
    // collapse to one address whenever an operator points two at the same
    // number, and a repeated entry in --allow-net is noise in the one place
    // somebody reads to find out what was granted.
    assert_eq!(
        net_allowlist("127.0.0.1", 3010, &[3011, 3012], ""),
        vec!["127.0.0.1:3010", "127.0.0.1:3011", "127.0.0.1:3012"]
    );
    assert_eq!(
        net_allowlist("127.0.0.1", 3010, &[3010, 3011, 3011], ""),
        vec!["127.0.0.1:3010", "127.0.0.1:3011"]
    );
}

#[test]
fn the_tracker_port_falls_back_to_the_config_ts_default() {
    // Must match `trackerPort` in be/src/config.ts. A launcher that grants
    // 3012 while the child binds something else is a backend that will not
    // boot under Deno, and the mismatch is invisible under Node until a click
    // arrives at a port nothing is listening on.
    assert_eq!(tracker_port(missing()), 3012);
    assert_eq!(hooks_port(missing()), 3011);
}

#[test]
fn one_port_serving_both_is_granted_once() {
    // Not a configuration to encourage — it would put the whole API behind the
    // tunnel — but a duplicate entry in a permission list is noise that makes
    // the real grant harder to read.
    assert_eq!(net_allowlist("127.0.0.1", 3010, &[3010], ""), vec!["127.0.0.1:3010"]);
}

#[test]
fn hooks_port_falls_back_to_the_config_ts_default() {
    // Same pairing as bind_address: the launcher grants the port the backend
    // will actually bind, and the two defaults live in different languages.
    assert_eq!(hooks_port(missing()), 3011);
}
