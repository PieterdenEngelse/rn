//! The regression tests for the rule that matters most: a user's environment
//! cannot reach the Node process.
//!
//! These test behaviour, not code shape. The clippy lint in clippy.toml checks
//! that nothing else calls `Command::new`; these check that the seal actually
//! holds, so they survive a refactor that legitimately restructures spawning.

use rn::node_command::NodeCommand;
use rn::settings::{resolve, RuntimeParam, Settings};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Serialises every test that touches the process environment.
///
/// `set_var` and `remove_var` are process-global and cargo runs a binary's
/// tests in parallel threads, so the one test that has to poison the ambient
/// environment would otherwise be mutating it underneath the three that spawn
/// Node — each of which reads the environment on its way (`NodeCommand::new`
/// looks up HOME). Rust marks these functions unsafe from the 2024 edition for
/// exactly this reason.
///
/// The sibling of this hazard in `runtime_argv.rs` was not theoretical: two
/// tests racing on `RN_PID_FILE` read and overwrote the user's real pidfile in
/// about a third of runs.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    // A test that panicked while holding it poisoned nothing we care about —
    // `Poisoned` restores the variable on unwind — so carry on rather than
    // failing every later test with a lock error.
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A process-global variable that removes itself again, panic or not.
///
/// Without the `Drop`, a failing assertion between `set_var` and `remove_var`
/// leaves the poison in the environment for every test that follows. Nothing
/// here would notice — they all spawn through `NodeCommand`, which clears the
/// environment — which is precisely what makes it worth closing: the next test
/// added to this file might not.
struct Poisoned {
    key: &'static str,
    _guard: MutexGuard<'static, ()>,
}

impl Poisoned {
    fn set(key: &'static str, value: &str) -> Self {
        let guard = env_lock();
        std::env::set_var(key, value);
        Self { key, _guard: guard }
    }
}

impl Drop for Poisoned {
    fn drop(&mut self) {
        std::env::remove_var(self.key);
    }
}

fn node_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("launcher/ has a parent")
        .join("be/runtime/bin/node")
}

fn install_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn user_node_options_cannot_reach_the_child() {
    let node = node_binary();
    if !node.is_file() {
        eprintln!("skipping: no runtime at {} — run scripts/install-node.sh", node.display());
        return;
    }

    // Exactly the thing that kills a process before its first line runs.
    // Held for the whole spawn, and removed on drop even if an assertion below
    // panics.
    let _poison = Poisoned::set("NODE_OPTIONS", "--require=/nonexistent/thing.js");

    let out = NodeCommand::new(&node, &install_dir())
        .arg("-e")
        .arg("process.stdout.write(process.env.NODE_OPTIONS ?? 'unset')")
        .output()
        .expect("node must start despite a poisoned parent environment");

    assert!(
        out.status.success(),
        "child failed to start: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "unset");
}

#[test]
fn our_node_options_do_reach_the_child() {
    let node = node_binary();
    if !node.is_file() {
        return;
    }
    // Reads the environment on the way (HOME), so it must not run while the
    // test above is mutating it.
    let _lock = env_lock();
    let out = NodeCommand::new(&node, &install_dir())
        .env("NODE_OPTIONS", "--max-old-space-size=256")
        .arg("-e")
        .arg("process.stdout.write(process.env.NODE_OPTIONS ?? 'unset')")
        .output()
        .expect("node runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "--max-old-space-size=256");
}

#[test]
fn the_child_is_marked_as_sealed() {
    let node = node_binary();
    if !node.is_file() {
        return;
    }
    // Reads the environment on the way (HOME), so it must not run while the
    // test above is mutating it.
    let _lock = env_lock();
    let out = NodeCommand::new(&node, &install_dir())
        .arg("-e")
        .arg("process.stdout.write(process.env.RN_ENV_SEALED ?? 'missing')")
        .output()
        .expect("node runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1");
}

fn param(id: &str, flag: &str, kind: &str, value_type: &str) -> RuntimeParam {
    serde_json::from_value(serde_json::json!({
        "id": id, "flag": flag, "kind": kind, "type": value_type,
    }))
    .expect("valid param")
}

#[test]
fn settings_resolve_to_env_vars_and_node_options() {
    let params = vec![
        param("threadpoolSize", "UV_THREADPOOL_SIZE", "env", "int"),
        param("maxOldSpaceSize", "--max-old-space-size", "node-option", "int"),
        param("traceWarnings", "--trace-warnings", "node-option", "bool"),
        param("timezone", "TZ", "env", "string"),
    ];
    let mut settings = Settings::new();
    settings.insert("threadpoolSize".into(), serde_json::json!(16));
    settings.insert("maxOldSpaceSize".into(), serde_json::json!(512));
    settings.insert("traceWarnings".into(), serde_json::json!(true));
    settings.insert("timezone".into(), serde_json::json!("Europe/Amsterdam"));

    let l = resolve(&params, &settings, "node");
    let (env, opts) = (l.env, l.node_options);

    assert_eq!(env.get("UV_THREADPOOL_SIZE").map(String::as_str), Some("16"));
    // A string value must not arrive JSON-quoted.
    assert_eq!(env.get("TZ").map(String::as_str), Some("Europe/Amsterdam"));
    assert!(opts.contains(&"--max-old-space-size=512".to_string()));
    assert!(opts.contains(&"--trace-warnings".to_string()));
}

#[test]
fn a_false_boolean_is_omitted_rather_than_passed_as_off() {
    let params = vec![param("traceWarnings", "--trace-warnings", "node-option", "bool")];
    let mut settings = Settings::new();
    settings.insert("traceWarnings".into(), serde_json::json!(false));

    let opts = resolve(&params, &settings, "node").node_options;
    assert!(opts.is_empty(), "there is no --no-trace-warnings form to pass");
}

#[test]
fn an_app_kind_param_never_reaches_node_options() {
    // The trap this guards: resolve() falls through to NODE_OPTIONS for any
    // kind it does not recognise. An app-kind param has no flag to give away
    // that it is not one, so an unhandled kind would emit
    // NODE_OPTIONS="--schedulerTickMs=30000" and Node would refuse to start —
    // the app would not boot, and the cause would be a settings row.
    let params = vec![RuntimeParam {
        id: "schedulerTickMs".to_string(),
        flag: "schedulerTickMs".to_string(),
        kind: "app".to_string(),
        value_type: "int".to_string(),
        applies_to: None,
        engine: None,
    }];
    let mut settings = Settings::new();
    settings.insert("schedulerTickMs".to_string(), serde_json::json!(5000));

    let launch = resolve(&params, &settings, "node");

    assert!(launch.node_options.is_empty(), "must not become a Node flag");
    assert!(launch.env.is_empty(), "must not become an environment variable");
    assert!(launch.runtime_flags.is_empty());
    assert!(launch.v8_flags.is_empty());
}
