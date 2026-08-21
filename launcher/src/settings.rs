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
pub fn resolve(params: &[RuntimeParam], settings: &Settings) -> (BTreeMap<String, String>, Vec<String>) {
    let mut env = BTreeMap::new();
    let mut node_options = Vec::new();

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

        if p.kind == "env" {
            let v = if p.value_type == "bool" { "1".to_string() } else { rendered };
            env.insert(p.flag.clone(), v);
        } else if p.value_type == "bool" {
            node_options.push(p.flag.clone());
        } else {
            node_options.push(format!("{}={}", p.flag, rendered));
        }
    }

    (env, node_options)
}
