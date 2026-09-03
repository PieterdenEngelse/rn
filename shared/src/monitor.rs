//! Everything the Monitor → Runtime page reads, and the rolling history behind
//! its charts.
//!
//! Unlike `jobs`, these carry explicit per-field renames rather than
//! `rename_all = "camelCase"`. The wire names are not all camelCase — the
//! memory figures end in `MB`, which `rename_all` would render `Mb` — and they
//! are not free to change: `~/.config/rn/history.json` stores samples under
//! these exact keys, so tidying them would silently orphan every reading a
//! user has accumulated.

use crate::wire;

wire! {
    /// One V8 heap space that currently holds something.
    pub struct HeapSpace {
        pub name: String,
        #[serde(rename = "usedMB")] pub used_mb: f64,
        #[serde(rename = "sizeMB")] pub size_mb: f64,
    }
}

wire! {
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
}

wire! {
    pub struct LargestSpace {
        pub name: String,
        #[serde(rename = "usedMB")] pub used_mb: f64,
    }
}

wire! {
    pub struct NodeEventLoop {
        #[serde(rename = "meanMs")] pub mean_ms: f64,
        #[serde(rename = "p50Ms")] pub p50_ms: f64,
        #[serde(rename = "p99Ms")] pub p99_ms: f64,
        #[serde(rename = "maxMs")] pub max_ms: f64,
        #[serde(rename = "utilizationPct")] pub utilization_pct: f64,
    }
}

wire! {
    pub struct NodeCpu {
        #[serde(rename = "userPct")] pub user_pct: f64,
        #[serde(rename = "systemPct")] pub system_pct: f64,
        pub cores: u32,
        pub load1: f64,
    }
}

wire! {
    pub struct NodeConcurrency {
        #[serde(rename = "threadpoolSize")] pub threadpool_size: u32,
        #[serde(rename = "activeResources")] pub active_resources: std::collections::BTreeMap<String, u32>,
        /// One entry per live handle, where the runtime can name them individually.
        /// Defaulted: an older backend sends counts and no detail, and that must
        /// leave the counts on screen rather than blanking the page.
        #[serde(default)] pub handles: Vec<HandleDetail>,
    }
}

wire! {
    /// One live handle and whatever distinguishes it from the others of its kind.
    ///
    /// `kind` uses the same vocabulary as the keys of `active_resources`, so a
    /// detail row can be filed under the count it belongs to. The backend does that
    /// translation — the two Node APIs behind these disagree on naming.
    pub struct HandleDetail {
        pub kind: String,
        /// Address, interval or pid — empty when nothing sets this handle apart.
        pub detail: String,
        /// Absent where the handle has no file descriptor, which is not the same
        /// as descriptor zero.
        // f64, not u64. ts-rs maps a 64-bit integer to `bigint`, which is
        // correct for Rust and wrong for this wire: the value arrives as a
        // JSON number from JavaScript, where it never was a bigint and cannot
        // become one. The mismatch was invisible while both ends were
        // hand-written.
        pub fd: Option<f64>,
    }
}

wire! {
    pub struct NodeHost {
        #[serde(rename = "totalMemMB")] pub total_mem_mb: f64,
        #[serde(rename = "freeMemMB")] pub free_mem_mb: f64,
    }
}

wire! {
    /// JavaScriptCore's own accounting, which has no Node equivalent.
    pub struct BunMetrics {
        #[serde(rename = "heapSizeMB")] pub heap_size_mb: f64,
        #[serde(rename = "heapCapacityMB")] pub heap_capacity_mb: f64,
        #[serde(rename = "objectCount")] pub object_count: f64,
        #[serde(rename = "protectedObjectCount")] pub protected_object_count: f64,
        #[serde(rename = "allocCurrentMB")] pub alloc_current_mb: f64,
        #[serde(rename = "allocPeakMB")] pub alloc_peak_mb: f64,
    }
}

wire! {
    /// What Deno is permitted to do — the only runtime that can answer this.
    pub struct DenoMetrics {
        pub permissions: std::collections::BTreeMap<String, String>,
        #[serde(rename = "bindAddressAllowed")] pub bind_address_allowed: bool,
    }
}

wire! {
    /// One figure that is not being measured, and why.
    ///
    /// `kind` is what decides the wording: a `runtime` gap is a consequence of the
    /// runtime selected on Config and can be undone by selecting another, while a
    /// `platform` gap is a fact about the machine with no action attached. Saying
    /// "not reported" for both would flatten two different next steps into one.
    pub struct Unavailable {
        pub id: String,
        pub kind: String,
        pub reason: String,
    }
}

wire! {
    /// Kernel counters, reported by all three runtimes.
    pub struct NodeResources {
        #[serde(rename = "maxRssMB")] pub max_rss_mb: f64,
        #[serde(rename = "fsRead")] pub fs_read: f64,
        #[serde(rename = "fsWrite")] pub fs_write: f64,
        #[serde(rename = "ctxVoluntary")] pub ctx_voluntary: f64,
        #[serde(rename = "ctxInvoluntary")] pub ctx_involuntary: f64,
        /// Milliseconds per second spent ready to run and waiting for a CPU. Read
        /// beside event-loop delay: it is what separates "my code blocked" from
        /// "this process could not get a core", which the delay figure alone
        /// cannot say. 0 where the kernel does not report it.
        #[serde(default, rename = "runqueueWaitMsPerSec")] pub runqueue_wait_ms_per_sec: f64,
    }
}

wire! {
    pub struct NodeGc {
        pub count: f64,
        #[serde(rename = "totalMs")] pub total_ms: f64,
    }
}

wire! {
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
}

wire! {
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
        /// Context switches per second over this interval, by kind. Voluntary is
        /// the process parking itself until something arrives; forced is the
        /// scheduler taking the CPU away for somebody else. None on samples stored
        /// before the two were counted apart — those carried only their sum, and a
        /// sum cannot be split afterwards.
        #[serde(default, rename = "ctxVolPerSec")] pub ctx_vol_per_sec: Option<f64>,
        #[serde(default, rename = "ctxInvolPerSec")] pub ctx_invol_per_sec: Option<f64>,
        /// Memory free on the machine, MB. None on samples stored before this was
        /// recorded.
        #[serde(default, rename = "hostFreeMB")] pub host_free_mb: Option<f64>,
        /// Runtime that measured it. None on samples stored before the tag existed.
        #[serde(default)] pub rt: Option<String>,
    }
}

wire! {
    pub struct LoopPercentile {
        pub label: String,
        pub ms: f64,
    }
}

wire! {
    /// One bucket of a long-window tier.
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
        /// Busiest second of context switching in the bucket, both kinds together.
        /// Kept beside the split rather than derived from it: the busiest second
        /// overall need not be the second either kind peaked in, so adding the two
        /// peaks would overstate it. It is also all a bucket recorded before the
        /// split has, and the widest tier keeps those for a year.
        #[serde(default, rename = "ctxPeakPerSec")] pub ctx_peak_per_sec: Option<f64>,
        /// The same peak per kind. None on buckets recorded before the split, which
        /// is why the chart draws the total for those and the two lines after.
        #[serde(default, rename = "ctxVolPeakPerSec")] pub ctx_vol_peak_per_sec: Option<f64>,
        #[serde(default, rename = "ctxInvolPeakPerSec")] pub ctx_invol_peak_per_sec: Option<f64>,
        /// Least free memory the machine had in the bucket.
        #[serde(default, rename = "hostFreeFloorMB")] pub host_free_floor_mb: Option<f64>,
        /// Fine samples that landed in it.
        pub n: f64,
        /// Runtime that measured it — the last one to write into it, when a restart
        /// swapped runtimes mid-bucket. None on buckets stored before the tag.
        #[serde(default)] pub rt: Option<String>,
    }
}

wire! {
    pub struct HistoryTier {
        pub id: String,
        pub label: String,
        #[serde(rename = "bucketMs")] pub bucket_ms: f64,
        pub capacity: u32,
        pub buckets: Vec<Bucket>,
    }
}

wire! {
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
}
