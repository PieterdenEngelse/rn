//! rn — the installed entry point.
//!
//! Owns the private Node runtime and supervises it. Everything it does exists
//! because of one rule from CLAUDE.md: the app never uses whatever Node is on
//! the machine, and never inherits the user's environment.

use rn::credentials;
use rn::layout::{self, display_path, display_text, Layout};
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
        match pidfile::running() {
            Some(rec) => {
                print!("rn: running (pid {})", rec.pid);
                if rec.started_at > 0 {
                    print!(", up {}", format_uptime(rec.started_at));
                }
                // Two different reasons the question cannot be answered, and
                // they send the reader to different places: a pidfile written
                // before provenance existed, versus a platform that will never
                // report it. Saying "this platform" for the first would be a
                // lie that survives until the next restart.
                if rec.parent_pid == 0 {
                    println!(
                        " (started before rn recorded provenance — restart it and \
                         `--status` will say who started it)"
                    );
                    return Ok(());
                }
                match pidfile::is_orphaned(&rec) {
                    // The case this exists for: whatever started rn has since
                    // exited, so nobody at the keyboard now has any reason to
                    // know this is running — and it will keep supervising the
                    // backend until someone stops it by name.
                    Some(true) => println!(
                        ", ORPHANED — the process that started it (pid {}) is gone. \
                         Stop it with `rn --stop` if you did not expect it.",
                        rec.parent_pid
                    ),
                    Some(false) => println!(", started by pid {}", rec.parent_pid),
                    None => println!(" (cannot tell what started it on this platform)"),
                }
            }
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
        display_path(&layout.node),
    );

    shutdown::install()?;
    pidfile::write_own()?;

    // Provenance in the log as well as the pidfile: the log is what survives in
    // a task output file or a journal, and "who started this" is the question
    // asked long after the fact.
    println!(
        "rn: pid {} started by pid {}",
        std::process::id(),
        pidfile::live_parent_of(std::process::id() as i32).unwrap_or(0),
    );

    let result = supervise(&layout, &params);
    pidfile::remove();
    result
}

fn print_help() {
    println!(
        "rn — automation runner

Usage: rn [options]

  --status      is rn running? Reports uptime and what started it.
  --stop        stop a running rn (and its Node process)
  --print-env   show the environment the Node process would get, and exit
  -h, --help    this text

The Node runtime ships with rn. Nothing here uses a Node from PATH."
    );
}

/// Where a Node option travels, which is not the same for every runtime.
///
/// Bun ignores NODE_OPTIONS outright — verified: --stack-trace-limit=42 set that
/// way leaves the limit at 10 — but accepts the same flags as argv, and tolerates
/// the ones it does not implement rather than refusing to start. Deno takes
/// neither: it rejects unknown arguments and would not boot, which is why no
/// Node option is ever routed to it.
///
/// Returns (NODE_OPTIONS entries, argv flags).
fn split_options(kind: layout::RuntimeKind, launch: &settings::Launch) -> (Vec<String>, Vec<String>) {
    let mut argv = launch.runtime_flags.clone();

    match kind {
        // Node takes both kinds in NODE_OPTIONS; V8 flags are accepted there.
        layout::RuntimeKind::Node => {
            let mut env = launch.node_options.clone();
            env.extend(launch.v8_flags.iter().cloned());
            (env, argv)
        }

        // Bun ignores NODE_OPTIONS and has no V8, so Node flags move to argv
        // and V8 flags are dropped — nothing there would read them.
        layout::RuntimeKind::Bun => {
            argv.extend(launch.node_options.iter().cloned());
            (Vec::new(), argv)
        }

        // Deno runs V8 but ignores NODE_OPTIONS, so its V8 flags travel in
        // --v8-flags. Node's own flags are dropped: Deno rejects arguments it
        // does not know and would refuse to start.
        //
        // The Deno V8 flags setting emits a --v8-flags of its own, so the two
        // are merged into one argument rather than passed twice, where the
        // second would silently replace the first.
        layout::RuntimeKind::Deno => {
            let mut v8: Vec<String> = Vec::new();
            argv.retain(|f| match f.strip_prefix("--v8-flags=") {
                Some(rest) => {
                    v8.extend(rest.split(',').map(str::trim).filter(|x| !x.is_empty()).map(String::from));
                    false
                }
                None => true,
            });
            v8.extend(launch.v8_flags.iter().cloned());
            if !v8.is_empty() {
                argv.push(format!("--v8-flags={}", v8.join(",")));
            }
            (Vec::new(), argv)
        }
    }
}

/// Build the child invocation. One place, so the supervisor and --print-env
/// can never disagree about what actually gets run.
fn build_command(layout: &Layout, params: &[settings::RuntimeParam]) -> NodeCommand {
    let saved = settings::load_settings(&layout.settings);
    let selection = layout.select_runtime(&saved);
    let launch = settings::resolve(params, &saved, selection.kind.name());
    let env = &launch.env;

    let mut cmd = NodeCommand::new(&selection.path, &layout.app_dir);
    cmd.envs(env);

    // Credentials, from outside the install tree. Passed through env() rather
    // than allowed through from the ambient environment: these are values we
    // read from a file we name, which is exactly the door env() is. The seal is
    // untouched. See launcher/src/credentials.rs and docs/sec.md.
    let creds = credentials::read_from(&credentials::path());
    cmd.envs(&creds.vars);
    for warning in &creds.warnings {
        // On stderr and not swallowed: a credential that looks configured and
        // is not is the failure the whole feature exists to make visible.
        eprintln!("rn: credentials: {warning}");
    }

    // The child reports these back through /api/params, so the Config page can
    // say what was asked for and what actually happened rather than leaving a
    // silently ignored setting on screen.
    cmd.env("RN_RUNTIME_PATH", &selection.path);
    cmd.env("RN_RUNTIME_REQUESTED", &selection.requested);
    if let Some(note) = &selection.unavailable {
        cmd.env("RN_RUNTIME_NOTE", note);
    }

    let (env_opts, argv_flags) = split_options(selection.kind, &launch);

    // Bun ignores NODE_OPTIONS outright — verified: --stack-trace-limit=42 set
    // that way leaves the limit at 10 — but accepts the same flags as argv, and
    // tolerates the ones it does not implement rather than refusing to start. So
    // under Bun they travel in the command line instead. Deno takes neither: it
    // rejects unknown arguments and would not boot, which is why nothing routed
    // here reaches it.
    if !env_opts.is_empty() {
        cmd.env("NODE_OPTIONS", env_opts.join(" "));
    }

    // Echoed so /api/params can tell a saved runtime flag from an applied one,
    // the same way NODE_OPTIONS lets it do that for Node flags.
    cmd.env("RN_RUNTIME_FLAGS", launch.runtime_flags.join(" "));

    // Pass-throughs, each deliberate:
    //   TERM       so the child can decide about colored output
    //   RN_*       our own configuration
    for key in [
        "TERM",
        "RN_SETTINGS_PATH",
        "RN_CORS_ORIGIN",
        "BACKEND_HOST",
        "BACKEND_PORT",
        // Without this the hooks listener silently falls back to 3011 in the
        // child while the launcher grants whatever the operator set — and under
        // Deno that mismatch is a backend that will not boot.
        "BACKEND_HOOKS_PORT",
    ] {
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
    let hooks_port = layout::hooks_port(&env_file);
    let allow_net = layout::net_allowlist(&host, port, hooks_port, &extra);

    // Echoed to the child so /api/params can report what was granted, and so a
    // saved-but-not-applied allowlist shows up in the restart banner.
    cmd.env("RN_NET_ALLOWLIST", allow_net.join(","));
    cmd.env("RN_NET_EXTRA", &extra);

    for a in layout::runtime_argv(
        selection.kind,
        &env_file,
        &layout.entry,
        &allow_net,
        &argv_flags,
    ) {
        cmd.arg(a);
    }
    cmd.current_dir(&layout.app_dir);
    cmd
}

fn print_env(layout: &Layout, params: &[settings::RuntimeParam]) -> Result<(), String> {
    let saved = settings::load_settings(&layout.settings);
    let selection = layout.select_runtime(&saved);
    let launch = settings::resolve(params, &saved, selection.kind.name());
    let env = &launch.env;

    let env_file = layout.app_dir.join(".env");
    let (host, port) = layout::bind_address(&env_file);
    let extra = saved.get("netAllowlist").and_then(|v| v.as_str()).unwrap_or_default();
    let hooks_port = layout::hooks_port(&env_file);
    let allow_net = layout::net_allowlist(&host, port, hooks_port, extra);
    let (env_opts, argv_flags) = split_options(selection.kind, &launch);
    let argv = layout::runtime_argv(
        selection.kind,
        &env_file,
        &layout.entry,
        &allow_net,
        &argv_flags,
    );
    println!(
        "runtime     {} -> {}",
        selection.requested,
        display_path(&selection.path),
    );
    println!(
        "argv        {}",
        // argv carries absolute paths to the child; shown tildified because
        // this line is for reading, not for pasting into a shell.
        argv.iter()
            .map(|a| display_text(&a.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    if let Some(note) = &selection.unavailable {
        println!("            ! {note}");
    }
    println!("node        {}", display_path(&layout.node));
    println!("entry       {}", display_path(&layout.entry));
    println!("settings    {}", display_path(&layout.settings));

    // Names only, never values — this is the command someone runs to check
    // their file is being read, often while someone else is looking at the
    // screen. See docs/sec.md.
    let creds_file = credentials::path();
    let creds = credentials::read_from(&creds_file);
    if creds.vars.is_empty() {
        println!("credentials {} (none set)", display_path(&creds_file));
    } else {
        println!(
            "credentials {} -> {}",
            display_path(&creds_file),
            creds.vars.keys().cloned().collect::<Vec<_>>().join(", "),
        );
    }
    for warning in &creds.warnings {
        println!("            ! {}", display_text(warning));
    }

    println!("sealed      RN_ENV_SEALED=1 (environment cleared, then built explicitly)");
    if env_opts.is_empty() {
        println!("NODE_OPTIONS  (none)");
    } else {
        println!("NODE_OPTIONS  {}", env_opts.join(" "));
    }
    for (k, v) in env {
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
            .map_err(|e| format!("cannot start {}: {e}", display_path(&layout.node)))?;

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

/// "21h 34m", from an epoch-millisecond start time.
fn format_uptime(started_at_ms: u128) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let secs = now.saturating_sub(started_at_ms) / 1000;
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3600, (secs % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}
