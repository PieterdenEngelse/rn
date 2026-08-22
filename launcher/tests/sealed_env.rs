//! The regression tests for the rule that matters most: a user's environment
//! cannot reach the Node process.
//!
//! These test behaviour, not code shape. The clippy lint in clippy.toml checks
//! that nothing else calls `Command::new`; these check that the seal actually
//! holds, so they survive a refactor that legitimately restructures spawning.

use rn::node_command::NodeCommand;
use rn::settings::{resolve, RuntimeParam, Settings};
use std::path::PathBuf;

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
    std::env::set_var("NODE_OPTIONS", "--require=/nonexistent/thing.js");

    let out = NodeCommand::new(&node, &install_dir())
        .arg("-e")
        .arg("process.stdout.write(process.env.NODE_OPTIONS ?? 'unset')")
        .output()
        .expect("node must start despite a poisoned parent environment");

    std::env::remove_var("NODE_OPTIONS");

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
