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

/// Why nothing answered, when nothing answered.
///
/// A failed fetch looks identical whether the port is closed, the process is
/// hung, or the browser refused to hand over a response that did arrive. The
/// distinction changes what you go and check, so it is worth establishing
/// rather than reporting "offline" and leaving it there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OfflineReason {
    /// Nothing accepted the connection: not running, or not on this port.
    NotListening,
    /// Something answered, but the browser would not release the response —
    /// almost always CORS, which means the backend is up and misconfigured.
    Blocked,
    /// The probe itself failed in a way we could not classify.
    Unknown,
}

impl OfflineReason {
    pub fn headline(self) -> &'static str {
        match self {
            OfflineReason::NotListening => "Nothing is listening",
            OfflineReason::Blocked => "Running, but the browser blocked the response",
            OfflineReason::Unknown => "Unreachable",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            OfflineReason::NotListening =>
                "The connection was refused outright, so no process holds this port. Either the backend is not running, or it is running on a different port than this page expects.",
            OfflineReason::Blocked =>
                "A second request without CORS enforcement did get a response, so the backend is up and reachable — the browser is discarding its answers. That is a CORS mismatch: the backend's allowed origin does not include the address this page was served from.",
            OfflineReason::Unknown =>
                "The request failed and the follow-up probe did not settle either way. Treat it as not running until something proves otherwise.",
        }
    }
}

/// Distinguish the cases above by asking again with CORS enforcement off. An
/// opaque response is still a response: it proves something accepted the
/// connection and replied, which a refused connection cannot do.
pub async fn diagnose_offline() -> OfflineReason {
    match gloo_net::http::Request::get(&format!("{API_BASE}/api/health"))
        .mode(web_sys::RequestMode::NoCors)
        .send()
        .await
    {
        Ok(_) => OfflineReason::Blocked,
        Err(gloo_net::Error::JsError(_)) => OfflineReason::NotListening,
        Err(_) => OfflineReason::Unknown,
    }
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

/// JavaScriptCore's own accounting, which has no Node equivalent.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BunMetrics {
    #[serde(rename = "heapSizeMB")] pub heap_size_mb: f64,
    #[serde(rename = "heapCapacityMB")] pub heap_capacity_mb: f64,
    #[serde(rename = "objectCount")] pub object_count: u64,
    #[serde(rename = "protectedObjectCount")] pub protected_object_count: u64,
    #[serde(rename = "allocCurrentMB")] pub alloc_current_mb: f64,
    #[serde(rename = "allocPeakMB")] pub alloc_peak_mb: f64,
}

/// What Deno is permitted to do — the only runtime that can answer this.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DenoMetrics {
    pub permissions: std::collections::BTreeMap<String, String>,
    #[serde(rename = "bindAddressAllowed")] pub bind_address_allowed: bool,
}

/// Kernel counters, reported by all three runtimes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeResources {
    #[serde(rename = "maxRssMB")] pub max_rss_mb: f64,
    #[serde(rename = "fsRead")] pub fs_read: u64,
    #[serde(rename = "fsWrite")] pub fs_write: u64,
    #[serde(rename = "ctxVoluntary")] pub ctx_voluntary: u64,
    #[serde(rename = "ctxInvoluntary")] pub ctx_involuntary: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeGc {
    pub count: u64,
    #[serde(rename = "totalMs")] pub total_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeMetrics {
    pub memory: NodeMemory,
    #[serde(rename = "eventLoop")] pub event_loop: NodeEventLoop,
    pub cpu: NodeCpu,
    pub resources: NodeResources,
    pub gc: NodeGc,
    pub concurrency: NodeConcurrency,
    pub host: NodeHost,
    pub versions: std::collections::BTreeMap<String, String>,
    /// Dotted paths this runtime does not actually count — see node_metrics.ts.
    #[serde(default)]
    pub unsupported: Vec<String>,
    /// Set when the runtime version differs from the one the list was probed on.
    #[serde(default, rename = "probeNote")]
    pub probe_note: Option<String>,
    /// Present only under the runtime that can report it.
    #[serde(default)]
    pub bun: Option<BunMetrics>,
    #[serde(default)]
    pub deno: Option<DenoMetrics>,
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

/// One bucket of a long-window tier.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Bucket {
    pub t: f64,
    #[serde(rename = "heapFloorMB")] pub heap_floor_mb: f64,
    #[serde(rename = "rssPeakMB")] pub rss_peak_mb: f64,
    #[serde(rename = "loopP99Ms")] pub loop_p99_ms: f64,
    #[serde(rename = "loopMaxMs")] pub loop_max_ms: f64,
    /// Fine samples that landed in it.
    pub n: u64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistoryTier {
    pub id: String,
    pub label: String,
    #[serde(rename = "bucketMs")] pub bucket_ms: f64,
    pub capacity: u32,
    pub buckets: Vec<Bucket>,
}

impl HistoryTier {
    /// How much of the window actually has data. Buckets are only written while
    /// rn is running, so a sparse tier is the normal case rather than a fault —
    /// but a chart drawn from 8 buckets looks exactly like one drawn from 365,
    /// so the number has to be stated.
    pub fn coverage_pct(&self) -> u32 {
        if self.capacity == 0 {
            return 0;
        }
        ((self.buckets.len() as f64 / self.capacity as f64) * 100.0).round() as u32
    }
}

/// One minute of the fine series, summarised — floor for heap, worst for the
/// rest. See node_history.ts for why neither is a mean.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CoarseSample {
    pub t: f64,
    #[serde(rename = "heapFloorMB")] pub heap_floor_mb: f64,
    #[serde(rename = "rssPeakMB")] pub rss_peak_mb: f64,
    #[serde(rename = "loopP99Ms")] pub loop_p99_ms: f64,
    #[serde(rename = "loopMaxMs")] pub loop_max_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeHistory {
    #[serde(rename = "sampleMs")] pub sample_ms: f64,
    pub capacity: u32,
    #[serde(rename = "heapLimitMB")] pub heap_limit_mb: f64,
    pub samples: Vec<HistorySample>,
    #[serde(rename = "loopPercentiles")] pub loop_percentiles: Vec<LoopPercentile>,
    /// Series this runtime does not measure; charting them would draw zeros.
    #[serde(default)]
    pub unsupported: Vec<String>,
    /// Epoch ms this process started.
    #[serde(default, rename = "startedAt")] pub started_at: f64,
    /// The longer windows: hour, day, week, month, year.
    #[serde(default)] pub tiers: Vec<HistoryTier>,
}

impl NodeHistory {
    pub fn measures(&self, series: &str) -> bool {
        !self.unsupported.iter().any(|u| u == series)
    }

    /// Where in the drawn series this process began, as a fraction of its
    /// width. Everything left of it was recorded by an earlier run, restored
    /// from disk — same numbers, different process, and worth a line saying so
    /// rather than a continuous curve implying one uninterrupted history.
    ///
    /// Computed from timestamps rather than from how full the buffer is: with
    /// samples restored, a short buffer no longer means a young process.
    pub fn before_start_fraction(&self) -> f64 {
        let n = self.samples.len();
        if n < 2 || self.started_at <= 0.0 {
            return 0.0;
        }
        match self.samples.iter().position(|s| s.t >= self.started_at) {
            // Every sample is from this run: nothing to mark.
            Some(0) => 0.0,
            Some(i) => (i as f64 / (n - 1) as f64).clamp(0.0, 1.0),
            // Every sample predates it, which should not happen — treat the
            // whole window as inherited rather than claiming it is current.
            None => 1.0,
        }
    }

    /// The same boundary within a tier's buckets.
    ///
    /// Only meaningful on the shorter tiers: over a week or a year almost every
    /// bucket predates the current process, so the marker would shade the whole
    /// chart and say nothing. Coverage is the honest measure at that length.
    pub fn tier_before_start_fraction(&self, tier: &HistoryTier) -> f64 {
        let n = tier.buckets.len();
        if n < 2 || self.started_at <= 0.0 || tier.bucket_ms > 900_000.0 {
            return 0.0;
        }
        match tier.buckets.iter().position(|b| b.t >= self.started_at) {
            Some(0) => 0.0,
            Some(i) => (i as f64 / (n - 1) as f64).clamp(0.0, 1.0),
            None => 0.0,
        }
    }
}

pub async fn fetch_node_history() -> Result<NodeHistory, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/node/history"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<NodeHistory>().await.map_err(|e| format!("{e}"))
}
