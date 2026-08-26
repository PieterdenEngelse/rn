//! The shapes the backend sends — data definitions and their serde attributes,
//! and nothing else.
//!
//! Deliberately logic-free. These are the types that move to the `shared/`
//! crate when it exists, at which point `fe` can no longer write `impl` blocks
//! for them; the behaviour that reads them already lives in [`super::history`]
//! as extension traits for exactly that reason. Keep it that way — a helper
//! added here is one that has to be moved again later.

use serde::Deserialize;

// The jobs surface is defined once, in the `shared` crate, and regenerated into
// be/src/generated/wire.ts for the Node side. Re-exported here so the rest of
// `fe` keeps importing from `crate::api`, and so the boundary is legible: what
// remains below is still hand-written on both ends, and is the next to move.
pub use shared::{
    CatalogueJob, ConnectionResponse, EnvEntry, HealthResponse, HooksHealth, EnvResponse, CredentialRef, JobErrors, JobInfo, JobInput, JobInputType,
    JobRun, JobRunResult, JobSource, JobStep, JobsConfig, JobsResponse, Outcome, RetryPolicy,
    RunningJob, RunsResponse, Schedule, ScheduledJob, Trigger,
};

// The monitor half of this file now lives in `shared`, which emits the
// TypeScript `be` imports — one definition, two ends, and a compile error
// rather than an `undefined` in a panel when a field is renamed. The config
// types below are the ones still hand-written on both sides.
pub use shared::monitor::*;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParamInfo {
    pub what: String,
    pub why: String,
    #[serde(rename = "ifWrong")]
    pub if_wrong: String,
}

/// One allowed value of an `enum` parameter.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParamOption {
    pub value: String,
    pub label: String,
    /// Present when the option explains itself; the UI prefers it over the
    /// parameter's own panel for the current selection.
    #[serde(default)]
    pub info: Option<ParamInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RuntimeParam {
    pub id: String,
    pub flag: String,
    pub kind: String,
    #[serde(rename = "type")]
    pub value_type: String,
    pub default: serde_json::Value,
    /// Key in the `effective` payload whose live value stands in for the
    /// default. Set where "the default" is whatever the OS reports, so the
    /// field can name it instead of just saying it is unset.
    #[serde(default, rename = "defaultFrom")]
    pub default_from: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub min: Option<i64>,
    #[serde(default)]
    pub max: Option<i64>,
    /// Present when `value_type` is "enum".
    #[serde(default)]
    pub options: Option<Vec<ParamOption>>,
    /// Runtimes this parameter does anything on. None means all of them.
    #[serde(default, rename = "appliesTo")]
    pub applies_to: Option<Vec<String>>,
    /// "v8" when the flag belongs to the engine rather than to a runtime.
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(rename = "appliesAt")]
    pub applies_at: String,
    pub category: String,
    pub label: String,
    pub info: ParamInfo,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Withheld {
    pub flag: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PendingChange {
    pub id: String,
    pub label: String,
    /// What the settings ask for.
    pub want: String,
    /// What the running process actually has.
    pub have: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ParamsResponse {
    pub params: Vec<RuntimeParam>,
    pub withheld: Vec<Withheld>,
    pub effective: serde_json::Value,
    pub settings: serde_json::Value,
    /// True when a launcher supervises the backend and can restart it.
    #[serde(default)]
    pub supervised: bool,
    /// Saved settings that are not in effect in the running process.
    #[serde(default)]
    pub pending: Vec<PendingChange>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaveResponse {
    pub ok: bool,
    #[serde(default, rename = "restartRequired")]
    pub restart_required: Vec<String>,
    #[serde(default)]
    pub errors: Vec<SaveError>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SaveError {
    pub id: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RestartOutcome {
    #[serde(default)]
    pub scheduled: bool,
    #[serde(default)]
    pub message: String,
    /// Jobs still running when a restart was queued behind them.
    #[serde(default)]
    pub running: Vec<RunningJob>,
    /// Jobs a `when=now` restart interrupted.
    #[serde(default)]
    pub aborted: Vec<RunningJob>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StatusResponse {
    pub supervised: bool,
    pub pid: u32,
    #[serde(rename = "launcherPid")]
    pub launcher_pid: Option<String>,
    #[serde(rename = "uptimeMs")]
    pub uptime_ms: f64,
    pub node: String,
    #[serde(rename = "execPath")]
    pub exec_path: String,
    #[serde(rename = "settingsPath")]
    pub settings_path: String,
    pub url: String,
    pub jobs: u32,
    #[serde(default, rename = "restartPending")]
    pub restart_pending: bool,
    /// Saved settings not yet in effect — drives the amber header light.
    #[serde(default, rename = "pendingCount")]
    pub pending_count: u32,
    /// Jobs whose most recent run failed — drives the red header light.
    /// Most-recent, not ever-failed, so a successful re-run clears it.
    #[serde(default, rename = "failedJobs")]
    pub failed_jobs: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StopOutcome {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub running: Vec<RunningJob>,
}
