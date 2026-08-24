//! Reading the history payload: the questions the charts need answered.
//!
//! These are extension traits rather than inherent `impl` blocks, and
//! deliberately so. The types they hang off are destined for the `shared/`
//! crate, and Rust's orphan rule means `fe` cannot write `impl NodeHistory`
//! once `NodeHistory` is defined elsewhere. A trait works either way, and
//! callers see no difference — method syntax still applies, they just need the
//! trait in scope.
//!
//! Nothing here talks to the network. It is all derivation from a payload that
//! has already arrived.

use super::wire::{HistoryTier, NodeHistory, Unavailable};

pub trait UnavailableView {
    /// The short label that stands in for the value.
    fn short(&self) -> &'static str;
}

impl UnavailableView for Unavailable {
    fn short(&self) -> &'static str {
        if self.kind == "platform" {
            "not on this platform"
        } else {
            "not measured here"
        }
    }
}

pub trait HistoryTierView {
    /// How much of the window actually has data. Buckets are only written while
    /// rn is running, so a sparse tier is the normal case rather than a fault —
    /// but a chart drawn from 8 buckets looks exactly like one drawn from 365,
    /// so the number has to be stated.
    fn coverage_pct(&self) -> u32;
}

impl HistoryTierView for HistoryTier {
    fn coverage_pct(&self) -> u32 {
        if self.capacity == 0 {
            return 0;
        }
        ((self.buckets.len() as f64 / self.capacity as f64) * 100.0).round() as u32
    }
}

pub trait NodeHistoryView {
    /// Whether the runtime running *now* measures a series. It says nothing
    /// about what the window already holds — see [`Self::has_loop_delay`].
    fn measures(&self, series: &str) -> bool;

    /// Why a series is missing, when it is.
    fn why_not(&self, series: &str) -> Option<&Unavailable>;

    /// Whether the fine window holds any event-loop reading at all.
    ///
    /// This is what decides whether the loop charts are drawn, rather than
    /// [`Self::measures`]. The two answer different questions and the window
    /// outlives the answer to the second: switch to Deno, which does not move
    /// the delay histogram, and the last five minutes still hold the real
    /// readings Node took before the restart. Hiding the chart because of what
    /// is running now would throw away data that was measured properly.
    fn has_loop_delay(&self) -> bool;

    /// The same question for a tier's buckets.
    fn tier_has_loop_delay(&self, tier: &HistoryTier) -> bool;

    /// Whether the window holds any run-queue reading. Same reasoning as
    /// [`Self::has_loop_delay`]: what the kernel reports now says nothing about
    /// what it reported while the window was being filled.
    fn has_cpu_wait(&self) -> bool;

    /// The same question for a tier's buckets.
    fn tier_has_cpu_wait(&self, tier: &HistoryTier) -> bool;

    /// Whether a tier holds any kernel rate — false for buckets recorded
    /// before they were sampled.
    fn tier_has_kernel(&self, tier: &HistoryTier) -> bool;

    /// Whether a tier holds any handle count. Same reasoning again: the chart
    /// follows what was recorded, not what the current runtime can record.
    fn tier_has_handles(&self, tier: &HistoryTier) -> bool;

    /// Where in the drawn series this process began, as a fraction of its
    /// width. Everything left of it was recorded by an earlier run, restored
    /// from disk — same numbers, different process, and worth a line saying so
    /// rather than a continuous curve implying one uninterrupted history.
    ///
    /// Computed from timestamps rather than from how full the buffer is: with
    /// samples restored, a short buffer no longer means a young process.
    fn before_start_fraction(&self) -> f64;

    /// Where in the drawn series the runtime last changed, as a fraction of its
    /// width, with the runtime that produced everything left of it.
    ///
    /// This is a different discontinuity from [`Self::before_start_fraction`],
    /// and the more serious one. A restart draws the same measurements from a
    /// new process; a runtime switch draws *different* measurements under the
    /// same names — heap used is V8's heap under Node and a compatibility shim
    /// over JavaScriptCore under Bun. The curve either side is not comparable,
    /// so the boundary is ruled and named rather than drawn through.
    ///
    /// Located at the last entry the running runtime did not write, so a window
    /// covering node → bun → node marks the whole stretch up to the return
    /// rather than only the middle of it. Entries stored before the tag existed
    /// count as foreign and are named "unknown", because they are.
    fn runtime_change(&self) -> Option<(f64, String)>;

    /// The runtime boundary within a tier's buckets.
    ///
    /// Marked on every tier, unlike the process-start rule: a restart happens
    /// often enough that on a long window it would shade everything, but a
    /// runtime switch is rare and stays worth pointing at a year later.
    fn tier_runtime_change(&self, tier: &HistoryTier) -> Option<(f64, String)>;

    /// The same boundary within a tier's buckets.
    ///
    /// Only meaningful on the shorter tiers: over a week or a year almost every
    /// bucket predates the current process, so the marker would shade the whole
    /// chart and say nothing. Coverage is the honest measure at that length.
    fn tier_before_start_fraction(&self, tier: &HistoryTier) -> f64;
}

impl NodeHistoryView for NodeHistory {
    fn measures(&self, series: &str) -> bool {
        self.why_not(series).is_none()
    }

    fn why_not(&self, series: &str) -> Option<&Unavailable> {
        self.unsupported.iter().find(|u| u.id == series)
    }

    fn has_loop_delay(&self) -> bool {
        self.samples.iter().any(|s| s.loop_p99_ms.is_some())
    }

    fn tier_has_loop_delay(&self, tier: &HistoryTier) -> bool {
        tier.buckets.iter().any(|b| b.loop_p99_ms.is_some())
    }

    fn has_cpu_wait(&self) -> bool {
        self.samples.iter().any(|s| s.cpu_wait_ms_per_sec.is_some())
    }

    fn tier_has_cpu_wait(&self, tier: &HistoryTier) -> bool {
        tier.buckets.iter().any(|b| b.cpu_wait_peak_ms_per_sec.is_some())
    }

    fn tier_has_kernel(&self, tier: &HistoryTier) -> bool {
        tier.buckets.iter().any(|b| b.fs_ops_peak_per_sec.is_some())
    }

    fn tier_has_handles(&self, tier: &HistoryTier) -> bool {
        tier.buckets.iter().any(|b| b.handles_peak.is_some())
    }

    fn before_start_fraction(&self) -> f64 {
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

    fn runtime_change(&self) -> Option<(f64, String)> {
        foreign_boundary(self.samples.len(), &self.runtime, |i| {
            self.samples[i].rt.as_deref()
        })
    }

    fn tier_runtime_change(&self, tier: &HistoryTier) -> Option<(f64, String)> {
        foreign_boundary(tier.buckets.len(), &self.runtime, |i| {
            tier.buckets[i].rt.as_deref()
        })
    }

    fn tier_before_start_fraction(&self, tier: &HistoryTier) -> f64 {
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

/// Shared by [`NodeHistoryView::runtime_change`] and its tier twin: scan from
/// the right for the last entry the running runtime did not write, and return
/// where it sits plus what wrote it.
///
/// An untagged entry counts as one of those, labelled "unknown" — history
/// written before the tag existed genuinely has no known origin, and it can
/// perfectly well have come from a different runtime. Skipping it would not
/// be neutral: an unbroken curve across it is itself a claim that the whole
/// window was measured the same way, which is the claim we cannot support.
/// Drawing the rule says only what is true, and it decays on its own as
/// tagged samples push it leftward out of the window.
fn foreign_boundary<'a>(
    n: usize,
    current: &str,
    rt: impl Fn(usize) -> Option<&'a str>,
) -> Option<(f64, String)> {
    // An empty `current` means a backend too old to report one. It cannot
    // tell us what is running, so it cannot tell us what is foreign either.
    if n < 2 || current.is_empty() {
        return None;
    }
    let i = (0..n).rev().find(|&i| rt(i) != Some(current))?;
    let previous = rt(i).unwrap_or("unknown").to_string();
    Some(((i as f64 / (n - 1) as f64).clamp(0.0, 1.0), previous))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Fixtures are deserialized from JSON rather than built as struct
    /// literals, so each one also exercises the `#[serde(rename)]` mapping the
    /// backend actually sends. A renamed field that stopped matching would
    /// surface here as a missing-field error instead of as `undefined` in a
    /// panel.
    fn history(v: serde_json::Value) -> NodeHistory {
        serde_json::from_value(v).expect("fixture should deserialize")
    }

    fn tier(v: serde_json::Value) -> HistoryTier {
        serde_json::from_value(v).expect("fixture should deserialize")
    }

    /// A sample carrying only what every sample must have, plus a runtime tag.
    fn sample(t: f64, rt: Option<&str>) -> serde_json::Value {
        match rt {
            Some(r) => json!({ "t": t, "heapUsedMB": 1.0, "rssMB": 2.0, "rt": r }),
            None => json!({ "t": t, "heapUsedMB": 1.0, "rssMB": 2.0 }),
        }
    }

    fn bucket(t: f64, rt: Option<&str>) -> serde_json::Value {
        match rt {
            Some(r) => json!({ "t": t, "heapFloorMB": 1.0, "rssPeakMB": 2.0, "n": 1, "rt": r }),
            None => json!({ "t": t, "heapFloorMB": 1.0, "rssPeakMB": 2.0, "n": 1 }),
        }
    }

    fn with_samples(samples: Vec<serde_json::Value>, runtime: &str, started_at: f64) -> NodeHistory {
        history(json!({
            "sampleMs": 1000.0,
            "capacity": 300,
            "heapLimitMB": 4096.0,
            "samples": samples,
            "loopPercentiles": [],
            "runtime": runtime,
            "startedAt": started_at,
        }))
    }

    // ---- Unavailable::short -------------------------------------------------

    #[test]
    fn short_distinguishes_platform_from_runtime() {
        let platform: Unavailable =
            serde_json::from_value(json!({ "id": "x", "kind": "platform", "reason": "r" })).unwrap();
        let runtime: Unavailable =
            serde_json::from_value(json!({ "id": "x", "kind": "runtime", "reason": "r" })).unwrap();

        // The two wordings exist because they imply different next steps: one
        // is undone by switching runtime on Config, the other never is.
        assert_eq!(platform.short(), "not on this platform");
        assert_eq!(runtime.short(), "not measured here");
    }

    // ---- HistoryTier::coverage_pct -----------------------------------------

    #[test]
    fn coverage_pct_reports_how_full_the_window_is() {
        let t = tier(json!({
            "id": "year", "label": "Year", "bucketMs": 86_400_000.0, "capacity": 365,
            "buckets": (0..8).map(|i| bucket(i as f64, None)).collect::<Vec<_>>(),
        }));
        // 8/365 = 2.19% — the point of the figure is that this chart looks
        // identical to a full one unless the number is stated.
        assert_eq!(t.coverage_pct(), 2);
    }

    #[test]
    fn coverage_pct_of_a_zero_capacity_tier_is_zero_not_a_panic() {
        let t = tier(json!({
            "id": "x", "label": "X", "bucketMs": 1000.0, "capacity": 0, "buckets": [],
        }));
        assert_eq!(t.coverage_pct(), 0);
    }

    // ---- measures / why_not / has_loop_delay -------------------------------

    #[test]
    fn has_loop_delay_outlives_measures() {
        // The invariant the doc comment argues for: the runtime running *now*
        // does not measure loop delay, but the window still holds readings a
        // previous runtime took. The chart must follow the data, not the
        // current capability, or a runtime switch silently discards history
        // that was measured properly.
        let h = history(json!({
            "sampleMs": 1000.0,
            "capacity": 300,
            "heapLimitMB": 4096.0,
            "samples": [
                { "t": 1.0, "heapUsedMB": 1.0, "rssMB": 2.0, "loopP99Ms": 3.5 },
                { "t": 2.0, "heapUsedMB": 1.0, "rssMB": 2.0 },
            ],
            "loopPercentiles": [],
            "unsupported": [
                { "id": "loopDelay", "kind": "runtime", "reason": "Deno has no histogram" }
            ],
        }));

        assert!(!h.measures("loopDelay"), "current runtime does not measure it");
        assert!(h.has_loop_delay(), "but the window holds a reading");
        assert_eq!(h.why_not("loopDelay").map(|u| u.kind.as_str()), Some("runtime"));
        assert!(h.why_not("somethingElse").is_none());
        assert!(h.measures("somethingElse"));
    }

    #[test]
    fn has_cpu_wait_follows_the_samples() {
        let none = with_samples(vec![sample(1.0, None), sample(2.0, None)], "node", 0.0);
        assert!(!none.has_cpu_wait());

        let some = history(json!({
            "sampleMs": 1000.0, "capacity": 300, "heapLimitMB": 4096.0,
            "samples": [{ "t": 1.0, "heapUsedMB": 1.0, "rssMB": 2.0, "cpuWaitMsPerSec": 12.0 }],
            "loopPercentiles": [],
        }));
        assert!(some.has_cpu_wait());
    }

    // ---- before_start_fraction ---------------------------------------------

    #[test]
    fn before_start_fraction_locates_this_process_in_the_window() {
        // 5 samples, this run began at the third: 2 of 4 gaps are inherited.
        let h = with_samples(
            vec![
                sample(0.0, None),
                sample(10.0, None),
                sample(20.0, None),
                sample(30.0, None),
                sample(40.0, None),
            ],
            "node",
            20.0,
        );
        assert_eq!(h.before_start_fraction(), 0.5);
    }

    #[test]
    fn before_start_fraction_is_zero_when_every_sample_is_from_this_run() {
        let h = with_samples(vec![sample(10.0, None), sample(20.0, None)], "node", 5.0);
        assert_eq!(h.before_start_fraction(), 0.0);
    }

    #[test]
    fn before_start_fraction_treats_an_all_inherited_window_as_fully_inherited() {
        // Should not happen, and the code deliberately claims the whole window
        // is inherited rather than claiming it is current.
        let h = with_samples(vec![sample(1.0, None), sample(2.0, None)], "node", 99.0);
        assert_eq!(h.before_start_fraction(), 1.0);
    }

    #[test]
    fn before_start_fraction_needs_two_samples_and_a_start_time() {
        assert_eq!(with_samples(vec![sample(1.0, None)], "node", 0.5).before_start_fraction(), 0.0);
        // startedAt absent (defaults to 0) means the backend did not report it.
        assert_eq!(
            with_samples(vec![sample(1.0, None), sample(2.0, None)], "node", 0.0)
                .before_start_fraction(),
            0.0
        );
    }

    // ---- runtime_change / foreign_boundary ---------------------------------

    #[test]
    fn runtime_change_marks_the_whole_stretch_up_to_the_return() {
        // node → bun → node. The boundary belongs at the *last* foreign entry
        // (index 2), not at the start of the bun run, so everything left of it
        // is marked — including the earlier node stretch, which is not
        // comparable across the gap either.
        let h = with_samples(
            vec![
                sample(0.0, Some("node")),
                sample(1.0, Some("bun")),
                sample(2.0, Some("bun")),
                sample(3.0, Some("node")),
                sample(4.0, Some("node")),
            ],
            "node",
            0.0,
        );
        assert_eq!(h.runtime_change(), Some((0.5, "bun".to_string())));
    }

    #[test]
    fn runtime_change_is_none_when_one_runtime_wrote_everything() {
        let h = with_samples(
            vec![sample(0.0, Some("node")), sample(1.0, Some("node"))],
            "node",
            0.0,
        );
        assert_eq!(h.runtime_change(), None);
    }

    #[test]
    fn an_untagged_sample_counts_as_foreign_and_is_named_unknown() {
        // History written before the tag existed has no known origin, and
        // drawing an unbroken curve across it would assert one.
        let h = with_samples(vec![sample(0.0, None), sample(1.0, Some("node"))], "node", 0.0);
        assert_eq!(h.runtime_change(), Some((0.0, "unknown".to_string())));
    }

    #[test]
    fn runtime_change_is_none_when_the_backend_reports_no_runtime() {
        // An empty `runtime` means a backend too old to say. It cannot tell us
        // what is running, so it cannot tell us what is foreign either.
        let h = with_samples(
            vec![sample(0.0, Some("node")), sample(1.0, Some("bun"))],
            "",
            0.0,
        );
        assert_eq!(h.runtime_change(), None);
    }

    #[test]
    fn runtime_change_needs_two_samples() {
        let h = with_samples(vec![sample(0.0, Some("bun"))], "node", 0.0);
        assert_eq!(h.runtime_change(), None);
    }

    // ---- tier variants ------------------------------------------------------

    #[test]
    fn tier_predicates_read_the_buckets_not_the_samples() {
        let h = with_samples(vec![sample(0.0, None)], "node", 0.0);
        let empty = tier(json!({
            "id": "hour", "label": "Hour", "bucketMs": 60_000.0, "capacity": 60,
            "buckets": [bucket(0.0, None), bucket(1.0, None)],
        }));
        assert!(!h.tier_has_loop_delay(&empty));
        assert!(!h.tier_has_cpu_wait(&empty));
        assert!(!h.tier_has_kernel(&empty));
        assert!(!h.tier_has_handles(&empty));

        let full = tier(json!({
            "id": "hour", "label": "Hour", "bucketMs": 60_000.0, "capacity": 60,
            "buckets": [{
                "t": 0.0, "heapFloorMB": 1.0, "rssPeakMB": 2.0, "n": 1,
                "loopP99Ms": 4.0, "cpuWaitPeakMsPerSec": 3.0,
                "fsOpsPeakPerSec": 10.0, "handlesPeak": 7.0,
            }],
        }));
        assert!(h.tier_has_loop_delay(&full));
        assert!(h.tier_has_cpu_wait(&full));
        assert!(h.tier_has_kernel(&full));
        assert!(h.tier_has_handles(&full));
    }

    #[test]
    fn tier_runtime_change_is_marked_on_every_tier() {
        // Unlike the process-start rule below, a runtime switch stays worth
        // pointing at however long the window is.
        let h = with_samples(vec![sample(0.0, None)], "node", 0.0);
        let t = tier(json!({
            "id": "year", "label": "Year", "bucketMs": 86_400_000.0, "capacity": 365,
            "buckets": [bucket(0.0, Some("bun")), bucket(1.0, Some("node")),
                        bucket(2.0, Some("node"))],
        }));
        assert_eq!(h.tier_runtime_change(&t), Some((0.0, "bun".to_string())));
    }

    #[test]
    fn tier_before_start_fraction_is_suppressed_on_long_windows() {
        // Over a day-per-bucket window almost every bucket predates the current
        // process, so the marker would shade the whole chart and say nothing.
        let h = with_samples(vec![sample(0.0, None)], "node", 50.0);
        let long = tier(json!({
            "id": "year", "label": "Year", "bucketMs": 86_400_000.0, "capacity": 365,
            "buckets": [bucket(0.0, None), bucket(100.0, None)],
        }));
        assert_eq!(h.tier_before_start_fraction(&long), 0.0);

        // The same data in a short-bucket tier does get marked.
        let short = tier(json!({
            "id": "hour", "label": "Hour", "bucketMs": 60_000.0, "capacity": 60,
            "buckets": [bucket(0.0, None), bucket(100.0, None)],
        }));
        assert_eq!(h.tier_before_start_fraction(&short), 1.0);
    }

    #[test]
    fn tier_before_start_fraction_returns_zero_where_the_sample_version_returns_one() {
        // A deliberate asymmetry with before_start_fraction: on a tier whose
        // buckets all predate the process, the marker is dropped rather than
        // shading everything. Pinned because it reads like a bug otherwise.
        let h = with_samples(vec![sample(0.0, None)], "node", 999.0);
        let t = tier(json!({
            "id": "hour", "label": "Hour", "bucketMs": 60_000.0, "capacity": 60,
            "buckets": [bucket(0.0, None), bucket(1.0, None)],
        }));
        assert_eq!(h.tier_before_start_fraction(&t), 0.0);
        assert_eq!(h.before_start_fraction(), 0.0); // n < 2 guard, not the None arm
    }
}
