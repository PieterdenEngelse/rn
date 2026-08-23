use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// One entry of the generated registry (be/runtime-params.json), produced from
/// be/src/runtime-params.ts. The launcher reads it rather than carrying a
/// second copy of the same definitions in Rust.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeParam {
    pub id: String,
    pub flag: String,
    /// "env", "node-option", or "launcher".
    pub kind: String,
    #[serde(rename = "type")]
    pub value_type: String,
    /// Runtimes this parameter does anything on; None means all of them.
    #[serde(default, rename = "appliesTo")]
    pub applies_to: Option<Vec<String>>,
    /// "v8" when the flag belongs to the engine rather than to Node.
    #[serde(default)]
    pub engine: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ParamsFile {
    params: Vec<RuntimeParam>,
}

pub type Settings = BTreeMap<String, serde_json::Value>;

pub fn load_params(path: &Path) -> Result<Vec<RuntimeParam>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}\n  Generate it with: cd be && npm run params:build", path.display()))?;
    let parsed: ParamsFile =
        serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
    Ok(parsed.params)
}

/// Missing or corrupt settings must never stop the app from starting — fall
/// back to defaults and let the UI report it.
pub fn load_settings(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Settings::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// The launcher half of what `resolveLaunch()` does in be/src/settings.ts:
/// turn saved settings into environment variables and NODE_OPTIONS entries.
/// env vars, NODE_OPTIONS entries, and flags for the runtime's own argv.
pub struct Launch {
    pub env: BTreeMap<String, String>,
    pub node_options: Vec<String>,
    pub runtime_flags: Vec<String>,
    /// Flags belonging to V8 rather than Node. Kept apart because the three
    /// runtimes take them by three different roads: Node accepts them inside
    /// NODE_OPTIONS, Deno needs them folded into --v8-flags, and Bun has no V8
    /// to give them to.
    pub v8_flags: Vec<String>,
}

/// `runtime` is what will actually be spawned. A parameter that does nothing on
/// it is dropped rather than passed and ignored: Bun and Deno tolerate a Node
/// flag, but Node does not tolerate --smol, so passing everything to everyone
/// would stop the default runtime booting.
pub fn resolve(params: &[RuntimeParam], settings: &Settings, runtime: &str) -> Launch {
    let mut env = BTreeMap::new();
    let mut node_options = Vec::new();
    let mut runtime_flags = Vec::new();
    let mut v8_flags = Vec::new();

    for p in params {
        let Some(value) = settings.get(&p.id) else { continue };
        if value.is_null() {
            continue;
        }
        // A false boolean means "absent", not "off" — there is no --no- form.
        if p.value_type == "bool" && value.as_bool() == Some(false) {
            continue;
        }

        let rendered = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };

        // A launcher-kind param is neither an env var nor a Node flag — the
        // launcher acts on it when choosing the binary. Falling through would
        // emit NODE_OPTIONS="runtime=node" and stop Node booting at all.
        if p.kind == "launcher" {
            continue;
        }

        if let Some(list) = &p.applies_to {
            if !list.iter().any(|r| r == runtime) {
                continue;
            }
        }

        // Not a Node flag: it belongs in the runtime's own command line, which
        // differs per runtime — see layout::runtime_argv.
        if p.kind == "runtime-flag" {
            if p.value_type == "bool" {
                runtime_flags.push(p.flag.clone());
            } else {
                runtime_flags.push(format!("{}={}", p.flag, rendered));
            }
            continue;
        }

        if p.engine.as_deref() == Some("v8") {
            v8_flags.push(format!("{}={}", p.flag, rendered));
            continue;
        }

        if p.kind == "env" {
            let v = if p.value_type == "bool" { "1".to_string() } else { rendered };
            env.insert(p.flag.clone(), v);
        } else if p.value_type == "bool" {
            node_options.push(p.flag.clone());
        } else {
            node_options.push(format!("{}={}", p.flag, rendered));
        }
    }

    Launch { env, node_options, runtime_flags, v8_flags }
}
