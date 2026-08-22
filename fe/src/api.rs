use serde::Deserialize;

/// Base URL of the backend API. In development the frontend is served by
/// `dx serve` on :1790 and the API lives on :3010, so this is absolute.
/// A packaged install serves both from one origin — see docs/packaging.md.
pub const API_BASE: &str = "http://127.0.0.1:3010";

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

pub async fn fetch_params() -> Result<ParamsResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/params"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if !resp.ok() {
        return Err(format!("backend returned {}", resp.status()));
    }
    resp.json::<ParamsResponse>().await.map_err(|e| format!("{e}"))
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

pub async fn save_settings(settings: serde_json::Value) -> Result<SaveResponse, String> {
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/settings"))
        .header("content-type", "application/json")
        .body(settings.to_string())
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    resp.json::<SaveResponse>().await.map_err(|e| format!("{e}"))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RunningJob {
    pub id: String,
    pub name: String,
    #[serde(rename = "startedAt")]
    pub started_at: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct JobsResponse {
    pub running: Vec<RunningJob>,
    #[serde(default, rename = "restartPending")]
    pub restart_pending: bool,
}

pub async fn fetch_jobs() -> Result<JobsResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/jobs"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<JobsResponse>().await.map_err(|e| format!("{e}"))
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

/// Ask the backend to restart. `now = false` waits for running work to finish.
/// Only works when a launcher supervises it — otherwise the backend answers
/// 409 rather than exiting into nothing.
pub async fn restart_backend(now: bool) -> Result<RestartOutcome, String> {
    let url = if now {
        format!("{API_BASE}/api/restart?when=now")
    } else {
        format!("{API_BASE}/api/restart")
    };
    let resp = gloo_net::http::Request::post(&url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<RestartOutcome>().await.map_err(|e| format!("{e}"));
    }
    // The 409 body explains why; surface that rather than a bare status code.
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("restart refused")
            .to_string()),
        Err(_) => Err(format!("restart failed ({})", resp.status())),
    }
}

/// Poll until the backend answers again, so the page can refresh once the new
/// process is actually up rather than guessing at a delay.
pub async fn wait_until_healthy(max_attempts: u32) -> bool {
    for _ in 0..max_attempts {
        gloo_timers::future::TimeoutFuture::new(500).await;
        if let Ok(resp) = gloo_net::http::Request::get(&format!("{API_BASE}/api/health"))
            .send()
            .await
        {
            if resp.ok() {
                return true;
            }
        }
    }
    false
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
}

pub async fn fetch_status() -> Result<StatusResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/status"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<StatusResponse>().await.map_err(|e| format!("{e}"))
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

/// Stop the backend. Refused with 409 while work is in progress unless forced.
pub async fn stop_backend(force: bool) -> Result<StopOutcome, String> {
    let url = if force {
        format!("{API_BASE}/api/stop?force=1")
    } else {
        format!("{API_BASE}/api/stop")
    };
    let resp = gloo_net::http::Request::post(&url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    let ok = resp.ok();
    let mut outcome = resp
        .json::<StopOutcome>()
        .await
        .map_err(|e| format!("{e}"))?;
    outcome.ok = ok;
    Ok(outcome)
}

/// Copy text to the clipboard. Fire-and-forget: the promise runs even though
/// we do not await it, and there is nothing useful to do if it is rejected.
pub fn copy_to_clipboard(text: &str) {
    if let Some(win) = web_sys::window() {
        let _ = win.navigator().clipboard().write_text(text);
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeMemory {
    #[serde(rename = "heapUsedMB")] pub heap_used_mb: f64,
    #[serde(rename = "heapTotalMB")] pub heap_total_mb: f64,
    #[serde(rename = "heapLimitMB")] pub heap_limit_mb: f64,
    #[serde(rename = "heapUsedPct")] pub heap_used_pct: f64,
    #[serde(rename = "rssMB")] pub rss_mb: f64,
    #[serde(rename = "externalMB")] pub external_mb: f64,
    #[serde(rename = "arrayBuffersMB")] pub array_buffers_mb: f64,
    #[serde(rename = "largestSpace")] pub largest_space: LargestSpace,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LargestSpace {
    pub name: String,
    #[serde(rename = "usedMB")] pub used_mb: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeEventLoop {
    #[serde(rename = "meanMs")] pub mean_ms: f64,
    #[serde(rename = "p50Ms")] pub p50_ms: f64,
    #[serde(rename = "p99Ms")] pub p99_ms: f64,
    #[serde(rename = "maxMs")] pub max_ms: f64,
    #[serde(rename = "utilizationPct")] pub utilization_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeCpu {
    #[serde(rename = "userPct")] pub user_pct: f64,
    #[serde(rename = "systemPct")] pub system_pct: f64,
    pub cores: u32,
    pub load1: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeConcurrency {
    #[serde(rename = "threadpoolSize")] pub threadpool_size: u32,
    #[serde(rename = "activeResources")] pub active_resources: std::collections::BTreeMap<String, u32>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeHost {
    #[serde(rename = "totalMemMB")] pub total_mem_mb: f64,
    #[serde(rename = "freeMemMB")] pub free_mem_mb: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeMetrics {
    pub memory: NodeMemory,
    #[serde(rename = "eventLoop")] pub event_loop: NodeEventLoop,
    pub cpu: NodeCpu,
    pub concurrency: NodeConcurrency,
    pub host: NodeHost,
    pub versions: std::collections::BTreeMap<String, String>,
    #[serde(rename = "uptimeMs")] pub uptime_ms: f64,
}

pub async fn fetch_node_metrics() -> Result<NodeMetrics, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/node"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<NodeMetrics>().await.map_err(|e| format!("{e}"))
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistorySample {
    pub t: f64,
    #[serde(rename = "heapUsedMB")] pub heap_used_mb: f64,
    #[serde(rename = "rssMB")] pub rss_mb: f64,
    #[serde(rename = "loopP50Ms")] pub loop_p50_ms: f64,
    #[serde(rename = "loopP99Ms")] pub loop_p99_ms: f64,
    #[serde(rename = "loopMaxMs")] pub loop_max_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LoopPercentile {
    pub label: String,
    pub ms: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeHistory {
    #[serde(rename = "sampleMs")] pub sample_ms: f64,
    pub capacity: u32,
    #[serde(rename = "heapLimitMB")] pub heap_limit_mb: f64,
    pub samples: Vec<HistorySample>,
    #[serde(rename = "loopPercentiles")] pub loop_percentiles: Vec<LoopPercentile>,
}

pub async fn fetch_node_history() -> Result<NodeHistory, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/node/history"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<NodeHistory>().await.map_err(|e| format!("{e}"))
}
