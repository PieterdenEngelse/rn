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
    CatalogueJob, JobErrors, JobInfo, JobRun, JobRunResult, JobSource, JobStep, JobsResponse,
    Outcome, RunningJob, Schedule, ScheduledJob, Trigger,
};

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

/// One V8 heap space that currently holds something.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HeapSpace {
    pub name: String,
    #[serde(rename = "usedMB")] pub used_mb: f64,
    #[serde(rename = "sizeMB")] pub size_mb: f64,
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
    /// Every space holding anything, largest first. Empty where unsupported.
    #[serde(default)] pub spaces: Vec<HeapSpace>,
    #[serde(rename = "largestSpace")] pub largest_space: LargestSpace,
    /// What old space may grow to, MB — derived, since V8 reports no per-space
    /// ceiling. See `oldSpaceMaxMB` in be/src/node_metrics.ts.
    #[serde(default, rename = "oldSpaceMaxMB")] pub old_space_max_mb: f64,
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

/// One figure that is not being measured, and why.
///
/// `kind` is what decides the wording: a `runtime` gap is a consequence of the
/// runtime selected on Config and can be undone by selecting another, while a
/// `platform` gap is a fact about the machine with no action attached. Saying
/// "not reported" for both would flatten two different next steps into one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Unavailable {
    pub id: String,
    pub kind: String,
    pub reason: String,
}

/// Kernel counters, reported by all three runtimes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeResources {
    #[serde(rename = "maxRssMB")] pub max_rss_mb: f64,
    #[serde(rename = "fsRead")] pub fs_read: u64,
    #[serde(rename = "fsWrite")] pub fs_write: u64,
    #[serde(rename = "ctxVoluntary")] pub ctx_voluntary: u64,
    #[serde(rename = "ctxInvoluntary")] pub ctx_involuntary: u64,
    /// Milliseconds per second spent ready to run and waiting for a CPU. Read
    /// beside event-loop delay: it is what separates "my code blocked" from
    /// "this process could not get a core", which the delay figure alone
    /// cannot say. 0 where the kernel does not report it.
    #[serde(default, rename = "runqueueWaitMsPerSec")] pub runqueue_wait_ms_per_sec: f64,
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
    /// Figures not being measured, each with the reason to show in place of it.
    #[serde(default)]
    pub unsupported: Vec<Unavailable>,
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistorySample {
    pub t: f64,
    #[serde(rename = "heapUsedMB")] pub heap_used_mb: f64,
    #[serde(rename = "rssMB")] pub rss_mb: f64,
    /// None when the runtime that took this sample does not measure loop
    /// delay. A gap, not a zero — the distinction survives on disk, so a window
    /// spanning a runtime switch keeps the readings that were real.
    #[serde(default, rename = "loopP50Ms")] pub loop_p50_ms: Option<f64>,
    #[serde(default, rename = "loopP99Ms")] pub loop_p99_ms: Option<f64>,
    #[serde(default, rename = "loopMaxMs")] pub loop_max_ms: Option<f64>,
    /// Milliseconds per second spent waiting for a core. None where the kernel
    /// does not report it, or on samples stored before this was recorded.
    #[serde(default, rename = "cpuWaitMsPerSec")] pub cpu_wait_ms_per_sec: Option<f64>,
    /// Resources keeping the process alive at this instant. None where the
    /// runtime does not report them, or on samples stored before this existed.
    #[serde(default)] pub handles: Option<f64>,
    /// Old space in use, MB. None where the runtime has no V8 spaces.
    #[serde(default, rename = "oldSpaceMB")] pub old_space_mb: Option<f64>,
    /// Filesystem operations per second over this interval. A rate, because the
    /// kernel's totals are per pid and restart at zero.
    #[serde(default, rename = "fsOpsPerSec")] pub fs_ops_per_sec: Option<f64>,
    /// Context switches per second over this interval.
    #[serde(default, rename = "ctxPerSec")] pub ctx_per_sec: Option<f64>,
    /// Memory free on the machine, MB. None on samples stored before this was
    /// recorded.
    #[serde(default, rename = "hostFreeMB")] pub host_free_mb: Option<f64>,
    /// Runtime that measured it. None on samples stored before the tag existed.
    #[serde(default)] pub rt: Option<String>,
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
    /// None when no sample in the bucket came from a runtime that measures it.
    #[serde(default, rename = "loopP99Ms")] pub loop_p99_ms: Option<f64>,
    #[serde(default, rename = "loopMaxMs")] pub loop_max_ms: Option<f64>,
    /// Worst run-queue wait in the bucket.
    #[serde(default, rename = "cpuWaitPeakMsPerSec")] pub cpu_wait_peak_ms_per_sec: Option<f64>,
    /// Most resources open at any sample in the bucket.
    #[serde(default, rename = "handlesPeak")] pub handles_peak: Option<f64>,
    /// Most old space held in the bucket.
    #[serde(default, rename = "oldSpacePeakMB")] pub old_space_peak_mb: Option<f64>,
    /// Busiest second of filesystem work in the bucket.
    #[serde(default, rename = "fsOpsPeakPerSec")] pub fs_ops_peak_per_sec: Option<f64>,
    /// Busiest second of context switching in the bucket.
    #[serde(default, rename = "ctxPeakPerSec")] pub ctx_peak_per_sec: Option<f64>,
    /// Least free memory the machine had in the bucket.
    #[serde(default, rename = "hostFreeFloorMB")] pub host_free_floor_mb: Option<f64>,
    /// Fine samples that landed in it.
    pub n: u64,
    /// Runtime that measured it — the last one to write into it, when a restart
    /// swapped runtimes mid-bucket. None on buckets stored before the tag.
    #[serde(default)] pub rt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HistoryTier {
    pub id: String,
    pub label: String,
    #[serde(rename = "bucketMs")] pub bucket_ms: f64,
    pub capacity: u32,
    pub buckets: Vec<Bucket>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeHistory {
    #[serde(rename = "sampleMs")] pub sample_ms: f64,
    pub capacity: u32,
    #[serde(rename = "heapLimitMB")] pub heap_limit_mb: f64,
    /// Installed memory, MB — constant, so the backend sends it once instead of
    /// putting it in every sample.
    #[serde(default, rename = "hostTotalMB")] pub host_total_mb: f64,
    pub samples: Vec<HistorySample>,
    #[serde(rename = "loopPercentiles")] pub loop_percentiles: Vec<LoopPercentile>,
    /// Series this runtime does not measure; charting them would draw zeros.
    #[serde(default)]
    pub unsupported: Vec<Unavailable>,
    /// Epoch ms this process started.
    #[serde(default, rename = "startedAt")] pub started_at: f64,
    /// The runtime answering right now, to read each entry's `rt` against.
    #[serde(default)] pub runtime: String,
    /// The longer windows: hour, day, week, month, year.
    #[serde(default)] pub tiers: Vec<HistoryTier>,
}
