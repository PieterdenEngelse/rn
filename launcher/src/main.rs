//! rn — the installed entry point.
//!
//! Owns the private Node runtime and supervises it. Everything it does exists
//! because of one rule from CLAUDE.md: the app never uses whatever Node is on
//! the machine, and never inherits the user's environment.

use rn::layout::{self, Layout};
use rn::node_command::NodeCommand;
use rn::pidfile;
use rn::settings;
use rn::shutdown;
use rn::EXIT_RESTART;
use std::time::{Duration, Instant};

/// Give up if the child dies this many times in quick succession — a crash loop
/// should surface as an error, not as an infinite restart.
const MAX_RAPID_RESTARTS: u32 = 5;
const RAPID_WINDOW: Duration = Duration::from_secs(10);

fn main() {
    if let Err(err) = run() {
        eprintln!("rn: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }

    // Process control first — these need no runtime.
    if args.iter().any(|a| a == "--stop") {
        return match pidfile::running_pid() {
            Some(pid) => {
                println!("rn: stopping pid {pid}");
                pidfile::stop(pid, Duration::from_secs(10))?;
                println!("rn: stopped");
                Ok(())
            }
            None => {
                println!("rn: not running");
                Ok(())
            }
        };
    }

    if args.iter().any(|a| a == "--status") {
        match pidfile::running_pid() {
            Some(pid) => println!("rn: running (pid {pid})"),
            None => println!("rn: not running"),
        }
        return Ok(());
    }

    let layout = Layout::resolve()?;
    let params = settings::load_params(&layout.params)?;

    if args.iter().any(|a| a == "--print-env") {
        return print_env(&layout, &params);
    }

    // Refuse to start a second copy: the child would fail to bind the port and
    // crash-loop, which is a confusing way to learn rn is already running.
    if let Some(pid) = pidfile::running_pid() {
        return Err(format!(
            "already running (pid {pid}). Stop it with `rn --stop`, or check `rn --status`.",
        ));
    }

    println!(
        "rn: {} runtime at {}",
        if layout.installed { "installed" } else { "development" },
        layout.node.display(),
    );

    shutdown::install()?;
    pidfile::write_own()?;

    let result = supervise(&layout, &params);
    pidfile::remove();
    result
}

fn print_help() {
    println!(
        "rn — automation runner

Usage: rn [options]

  --status      is rn running?
  --stop        stop a running rn (and its Node process)
  --print-env   show the environment the Node process would get, and exit
  -h, --help    this text

The Node runtime ships with rn. Nothing here uses a Node from PATH."
    );
}

/// Build the child invocation. One place, so the supervisor and --print-env
/// can never disagree about what actually gets run.
fn build_command(layout: &Layout, params: &[settings::RuntimeParam]) -> NodeCommand {
    let saved = settings::load_settings(&layout.settings);
    let (env, node_options) = settings::resolve(params, &saved);
    let selection = layout.select_runtime(&saved);

    let mut cmd = NodeCommand::new(&selection.path, &layout.app_dir);
    cmd.envs(&env);

    // The child reports these back through /api/params, so the Config page can
    // say what was asked for and what actually happened rather than leaving a
    // silently ignored setting on screen.
    cmd.env("RN_RUNTIME_PATH", &selection.path);
    cmd.env("RN_RUNTIME_REQUESTED", &selection.requested);
    if let Some(note) = &selection.unavailable {
        cmd.env("RN_RUNTIME_NOTE", note);
    }

    if !node_options.is_empty() {
        cmd.env("NODE_OPTIONS", node_options.join(" "));
    }

    // Pass-throughs, each deliberate:
    //   TERM       so the child can decide about colored output
    //   RN_*       our own configuration
    for key in ["TERM", "RN_SETTINGS_PATH", "RN_CORS_ORIGIN", "BACKEND_HOST", "BACKEND_PORT"] {
        cmd.allow_var(key);
    }

    // Each runtime takes a different command line — see runtime_argv.
    let env_file = layout.app_dir.join(".env");
    let (host, port) = layout::bind_address(&env_file);
    let extra = saved
        .get("netAllowlist")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let allow_net = layout::net_allowlist(&host, port, &extra);

    // Echoed to the child so /api/params can report what was granted, and so a
    // saved-but-not-applied allowlist shows up in the restart banner.
    cmd.env("RN_NET_ALLOWLIST", allow_net.join(","));
    cmd.env("RN_NET_EXTRA", &extra);

    for a in layout::runtime_argv(selection.kind, &env_file, &layout.entry, &allow_net) {
        cmd.arg(a);
    }
    cmd.current_dir(&layout.app_dir);
    cmd
}

fn print_env(layout: &Layout, params: &[settings::RuntimeParam]) -> Result<(), String> {
    let saved = settings::load_settings(&layout.settings);
    let (env, node_options) = settings::resolve(params, &saved);

    let selection = layout.select_runtime(&saved);
    let env_file = layout.app_dir.join(".env");
    let (host, port) = layout::bind_address(&env_file);
    let extra = saved.get("netAllowlist").and_then(|v| v.as_str()).unwrap_or_default();
    let allow_net = layout::net_allowlist(&host, port, extra);
    let argv = layout::runtime_argv(selection.kind, &env_file, &layout.entry, &allow_net);
    println!("runtime     {} -> {}", selection.requested, selection.path.display());
    println!(
        "argv        {}",
        argv.iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")
    );
    if let Some(note) = &selection.unavailable {
        println!("            ! {note}");
    }
    println!("node        {}", layout.node.display());
    println!("entry       {}", layout.entry.display());
    println!("settings    {}", layout.settings.display());
    println!("sealed      RN_ENV_SEALED=1 (environment cleared, then built explicitly)");
    if node_options.is_empty() {
        println!("NODE_OPTIONS  (none)");
    } else {
        println!("NODE_OPTIONS  {}", node_options.join(" "));
    }
    for (k, v) in &env {
        println!("env         {k}={v}");
    }
    Ok(())
}

fn supervise(layout: &Layout, params: &[settings::RuntimeParam]) -> Result<(), String> {
    let mut rapid = 0u32;
    let mut window_start = Instant::now();

    loop {
        let mut cmd = build_command(layout, params);
        let started = Instant::now();

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", layout.node.display()))?;

        shutdown::set_child(Some(child.id()));

        let status = shutdown::wait_for(&mut child)
            .map_err(|e| format!("lost track of the Node process: {e}"))?;

        shutdown::set_child(None);

        // An intentional stop is not a crash. Check before interpreting the
        // exit code, or Ctrl-C looks like a failure worth restarting.
        if shutdown::is_shutting_down() {
            println!("rn: stopped");
            return Ok(());
        }

        let code = status.code();

        match code {
            Some(EXIT_RESTART) => {
                // Asked for. Rebuild the environment from the settings as they
                // are now — that is the whole point of the restart.
                println!("rn: restarting to apply new settings");
                rapid = 0;
                continue;
            }
            Some(0) => {
                println!("rn: stopped");
                return Ok(());
            }
            other => {
                if started.elapsed() > RAPID_WINDOW {
                    rapid = 0;
                    window_start = Instant::now();
                }
                rapid += 1;
                if rapid >= MAX_RAPID_RESTARTS && window_start.elapsed() < RAPID_WINDOW * 3 {
                    return Err(format!(
                        "Node exited {} times in quick succession (last code {:?}). Not restarting again.\n  Run `rn --print-env` to see what it was started with.",
                        rapid, other,
                    ));
                }
                eprintln!("rn: Node exited with {other:?} — restarting ({rapid}/{MAX_RAPID_RESTARTS})");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}
