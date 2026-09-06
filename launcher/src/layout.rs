use std::path::{Path, PathBuf};

/// Where everything lives, resolved from the launcher's own location so a
/// moved or relocated install still works.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The private Node binary. Always absolute — never `node` from PATH.
    pub node: PathBuf,
    /// Directory containing the app sources.
    pub app_dir: PathBuf,
    /// Script Node is told to run.
    pub entry: PathBuf,
    /// Generated parameter registry, shared with the backend.
    pub params: PathBuf,
    /// User settings. Outside the install tree — that is replaced on upgrade.
    pub settings: PathBuf,
    /// True when running from a packaged install rather than the repo.
    pub installed: bool,
}

/// Which binary the launcher will actually spawn, and whether that is what the
/// settings asked for.
///
/// An unhonourable request is never fatal. A user who selects a runtime this
/// install does not carry gets the bundled one and a note saying so — refusing
/// to boot would leave them with no UI in which to change the setting back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Node,
    Bun,
    Deno,
}

impl RuntimeKind {
    /// Matches the values used in the registry's appliesTo.
    pub fn name(self) -> &'static str {
        match self {
            RuntimeKind::Node => "node",
            RuntimeKind::Bun => "bun",
            RuntimeKind::Deno => "deno",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeSelection {
    /// Absolute path to the binary to spawn.
    pub path: PathBuf,
    /// The kind of the binary at `path` — what will actually run, which is not
    /// the same as what was requested when a fallback happened. Argv is built
    /// from this, so a fallback to Node gets Node's argv and not Bun's.
    pub kind: RuntimeKind,
    /// What the settings asked for, for logging and for the UI.
    pub requested: String,
    /// Set when the request could not be honoured, explaining why.
    pub unavailable: Option<String>,
}

/// Read one key out of a .env file. Deliberately minimal — enough to mirror
/// what `config.ts` will see, not a general dotenv implementation.
fn env_file_value(env_file: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(env_file).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        if k.trim() == key {
            return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }
    None
}

/// The address the child will listen on, mirroring the precedence in
/// be/src/config.ts exactly: a real environment variable wins over .env, which
/// wins over the default. Getting this wrong means Deno denies the app its own
/// listening socket, so it has to agree with the backend rather than guess.
pub fn bind_address(env_file: &Path) -> (String, u16) {
    let host = std::env::var("BACKEND_HOST")
        .ok()
        .or_else(|| env_file_value(env_file, "BACKEND_HOST"))
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port = std::env::var("BACKEND_PORT")
        .ok()
        .or_else(|| env_file_value(env_file, "BACKEND_PORT"))
        .and_then(|p| p.parse().ok())
        .unwrap_or(3010);
    (host, port)
}

/// The hooks listener's port, mirroring `hooksPort` in be/src/config.ts by the
/// same precedence as `bind_address`.
///
/// A second function rather than a third element on the tuple above: callers
/// that only want the API address outnumber the ones that want both, and a
/// tuple that grows every time the backend opens a port is one every caller has
/// to be edited for.
pub fn hooks_port(env_file: &Path) -> u16 {
    std::env::var("BACKEND_HOOKS_PORT")
        .ok()
        .or_else(|| env_file_value(env_file, "BACKEND_HOOKS_PORT"))
        .and_then(|p| p.parse().ok())
        .unwrap_or(3011)
}

/// The click tracker's port, mirroring `trackerPort` in be/src/config.ts.
///
/// Same shape and same precedence as [`hooks_port`], and granted for the same
/// non-optional reason: the tracker binds at startup, so under Deno an
/// ungranted port is a backend that does not boot rather than a feature that
/// quietly does nothing. See docs/link-tracking.md §3.
pub fn tracker_port(env_file: &Path) -> u16 {
    std::env::var("BACKEND_TRACKER_PORT")
        .ok()
        .or_else(|| env_file_value(env_file, "BACKEND_TRACKER_PORT"))
        .and_then(|p| p.parse().ok())
        .unwrap_or(3012)
}

/// The hosts Deno may reach: the app's own listening sockets, plus whatever the
/// `netAllowlist` setting adds for job code that calls outward.
///
/// The bind address is always present — without it the server cannot listen at
/// all — so this never returns an empty list, and the broad `--allow-net`
/// fallback in `runtime_argv` stays unreachable in practice.
///
/// The listeners beside it are granted for the same reason and are not
/// optional: each binds at startup, so under Deno an ungranted port is not a
/// feature that quietly does nothing, it is a backend that fails to boot.
///
/// They arrive as a slice rather than as one parameter each. The signature was
/// `(host, port, hooks_port, extra)` while there was exactly one, and the next
/// listener would have made it four positional numbers whose order nothing
/// checks — the same argument [`hooks_port`] gives for not growing the tuple
/// [`bind_address`] returns.
pub fn net_allowlist(host: &str, port: u16, others: &[u16], extra: &str) -> Vec<String> {
    let mut out = vec![format!("{host}:{port}")];
    for other in others {
        let entry = format!("{host}:{other}");
        if !out.iter().any(|h| *h == entry) {
            out.push(entry);
        }
    }
    for host in extra.split(',').map(str::trim).filter(|h| !h.is_empty()) {
        if !out.iter().any(|h| h == host) {
            out.push(host.to_string());
        }
    }
    out
}

/// The command line for a given runtime. Each speaks a different dialect: the
/// script path is the only part all three agree on.
///
/// Pure and public so `--print-env` and the tests can see exactly what would be
/// run without spawning anything.
pub fn runtime_argv(
    kind: RuntimeKind,
    env_file: &Path,
    entry: &Path,
    allow_net: &[String],
    // Flags for this runtime's own command line, from runtime-flag params.
    extra: &[String],
) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;
    let mut argv: Vec<OsString> = Vec::new();

    match kind {
        RuntimeKind::Node => {
            // --env-file cannot go through NODE_OPTIONS: it is one of the
            // CLI-only flags. The -if-exists form tolerates a missing file.
            argv.push(OsString::from(format!("--env-file-if-exists={}", env_file.display())));
        }
        RuntimeKind::Bun => {
            // Bun reads .env from the working directory by itself, and its
            // --env-file has no "if exists" form — passing one that is missing
            // is an error. Relying on cwd avoids having to guess.
            argv.push(OsString::from("run"));
        }
        RuntimeKind::Deno => {
            argv.push(OsString::from("run"));
            // Deno denies everything by default, so the app's needs are named
            // one at a time. The network grant is the point of running here at
            // all: it lists the app's own bind address plus whatever the
            // netAllowlist setting adds, rather than opening the whole network.
            if allow_net.is_empty() {
                // Unreachable in practice — net_allowlist always includes the
                // bind address. Kept so a future caller cannot silently ship a
                // server that fails to listen.
                argv.push(OsString::from("--allow-net"));
            } else {
                argv.push(OsString::from(format!("--allow-net={}", allow_net.join(","))));
            }
            argv.push(OsString::from("--allow-env"));
            argv.push(OsString::from("--allow-read"));
            // The settings file is written from the backend, not only read.
            argv.push(OsString::from("--allow-write"));
            // node:v8 heap statistics and os.availableParallelism.
            argv.push(OsString::from("--allow-sys"));
            if env_file.is_file() {
                argv.push(OsString::from(format!("--env-file={}", env_file.display())));
            }
        }
    }

    // Before the script path: every one of the three parses options first and
    // treats the first non-option argument as the program to run.
    for f in extra {
        argv.push(OsString::from(f));
    }

    argv.push(entry.as_os_str().to_os_string());
    argv
}

/// Alternative runtimes sit beside the default one as `runtime-<id>/bin/<exe>`:
/// `runtime-bun/bin/bun`, `runtime-node20/bin/node`. The plain `runtime/` dir
/// stays what it has always been — the default bundled Node.
fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn node_binary_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

impl Layout {
    /// Installed layout: `<dir>/rn`, `<dir>/runtime/bin/node`, `<dir>/app/...`
    /// Development layout: the repo, with `be/` as the app directory.
    pub fn resolve() -> Result<Self, String> {
        let exe = std::env::current_exe().map_err(|e| format!("cannot locate own binary: {e}"))?;
        let exe_dir = exe
            .parent()
            .ok_or_else(|| "own binary has no parent directory".to_string())?;

        // Installed: runtime/ sits beside the launcher.
        let installed_runtime = exe_dir.join("runtime").join("bin").join(node_binary_name());
        if installed_runtime.is_file() {
            let app_dir = exe_dir.join("app");
            return Ok(Self {
                node: installed_runtime,
                entry: app_dir.join("src").join("server.ts"),
                params: app_dir.join("runtime-params.json"),
                app_dir,
                settings: settings_path(),
                installed: true,
            });
        }

        // Development: walk up until a directory containing be/ turns up.
        let mut dir: Option<&Path> = Some(exe_dir);
        while let Some(d) = dir {
            let be = d.join("be");
            if be.join("runtime").join("bin").join(node_binary_name()).is_file() {
                return Ok(Self {
                    node: be.join("runtime").join("bin").join(node_binary_name()),
                    entry: be.join("src").join("server.ts"),
                    params: be.join("runtime-params.json"),
                    app_dir: be,
                    settings: settings_path(),
                    installed: false,
                });
            }
            dir = d.parent();
        }

        Err(format!(
            "no Node runtime found.\n  Looked beside {} for runtime/bin/{}, then upward for be/runtime.\n  Install it with: scripts/install-node.sh",
            exe_dir.display(),
            node_binary_name(),
        ))
    }
}

impl Layout {
    /// Pick the runtime binary from the saved `jsRuntime` / `nodeVersion`
    /// settings, falling back to the bundled one when the request cannot be met.
    pub fn select_runtime(&self, settings: &crate::settings::Settings) -> RuntimeSelection {
        let runtime = settings
            .get("jsRuntime")
            .and_then(|v| v.as_str())
            .unwrap_or("node");
        let version = settings.get("nodeVersion").and_then(|v| v.as_str());

        // The common case: bundled Node, nothing to resolve.
        if runtime == "node" && version.is_none() {
            return RuntimeSelection {
                path: self.node.clone(),
                kind: RuntimeKind::Node,
                requested: "node (bundled)".to_string(),
                unavailable: None,
            };
        }

        let (dir, binary, label, kind) = match runtime {
            "bun" => ("runtime-bun".to_string(), exe_name("bun"), "bun".to_string(), RuntimeKind::Bun),
            "deno" => ("runtime-deno".to_string(), exe_name("deno"), "deno".to_string(), RuntimeKind::Deno),
            _ => {
                let v = version.unwrap_or_default();
                (
                    format!("runtime-node{v}"),
                    node_binary_name().to_string(),
                    format!("node {v}"),
                    RuntimeKind::Node,
                )
            }
        };

        // self.node is <root>/runtime/bin/<exe>; three parents up is <root>.
        let root = self
            .node
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent);

        let candidate = root.map(|r| r.join(&dir).join("bin").join(&binary));

        match candidate {
            Some(path) if path.is_file() => RuntimeSelection {
                path,
                kind,
                requested: label,
                unavailable: None,
            },
            // Falling back means Node runs, so the argv must be Node's.
            Some(path) => RuntimeSelection {
                path: self.node.clone(),
                kind: RuntimeKind::Node,
                requested: label.clone(),
                unavailable: Some(format!(
                    "{label} is not bundled with this install (looked for {}) — using the bundled Node instead",
                    path.display()
                )),
            },
            None => RuntimeSelection {
                path: self.node.clone(),
                kind: RuntimeKind::Node,
                requested: label.clone(),
                unavailable: Some(format!("cannot locate the runtime directory to look for {label}")),
            },
        }
    }
}

/// Must match `config.settingsPath` in be/src/config.ts.
fn settings_path() -> PathBuf {
    if let Ok(explicit) = std::env::var("RN_SETTINGS_PATH") {
        return PathBuf::from(explicit);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("rn").join("settings.json")
}

/// Format arbitrary text for display, shortening the home directory wherever
/// it appears — including inside a flag like `--env-file=/home/you/...`, which
/// a prefix match would miss.
pub fn display_text(text: &str) -> String {
    let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) else {
        return text.to_string();
    };
    if home.is_empty() || home == "/" {
        return text.to_string();
    }
    text.replace(&home, "~")
}

/// Format a path for showing to a person: home directory as `~`.
///
/// Display only. Everything that spawns, opens or compares a path keeps the
/// absolute form — `~` is a shell convention, not something the file system
/// resolves.
pub fn display_path(p: &Path) -> String {
    let text = p.display().to_string();
    let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) else {
        return text;
    };
    if home.is_empty() || home == "/" {
        return text;
    }
    if text == home {
        return "~".to_string();
    }
    match text.strip_prefix(&format!("{home}/")) {
        Some(rest) => format!("~/{rest}"),
        None => text,
    }
}
