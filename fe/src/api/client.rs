//! Talking to the backend: one function per endpoint, plus the diagnosis of
//! what it means when none of them answer.
//!
//! Everything here is `fe`-only and stays here — it depends on `gloo-net` and
//! `web-sys`, neither of which belongs anywhere near a shared type crate.

use super::wire::{
    ConnectionResponse, EnvResponse, HealthResponse, JobErrors, JobRunResult, JobSource, JobsResponse, NodeHistory, NodeMetrics,
    ParamsResponse, RestartOutcome, RunsResponse, SaveResponse, StateResetResponse, StatusResponse,
    StopOutcome,
};

/// Base URL of the backend API. In development the frontend is served by
/// `dx serve` on :1790 and the API lives on :3010, so this is absolute.
/// A packaged install serves both from one origin — see docs/packaging.md.
pub const API_BASE: &str = "http://127.0.0.1:3010";

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

pub async fn fetch_jobs() -> Result<JobsResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/jobs"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<JobsResponse>().await.map_err(|e| format!("{e}"))
}

/// What is listening, who may talk to it, and what it may reach.
pub async fn fetch_connection() -> Result<ConnectionResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/connection"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<ConnectionResponse>()
        .await
        .map_err(|e| format!("{e}"))
}

/// Liveness, and the state of the hooks listener.
///
/// Monitor → Connection reads this only as a fallback: the handle list is the
/// page's premise and answers the same question more directly. This is what it
/// falls back to where that list is empty — under Bun and Deno, which do not
/// implement the API it comes from.
pub async fn fetch_health() -> Result<HealthResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/health"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<HealthResponse>()
        .await
        .map_err(|e| format!("{e}"))
}

/// What be/.env says, against what the process actually has.
pub async fn fetch_env() -> Result<EnvResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/env"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<EnvResponse>().await.map_err(|e| format!("{e}"))
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

pub async fn fetch_node_metrics() -> Result<NodeMetrics, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/node"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<NodeMetrics>().await.map_err(|e| format!("{e}"))
}

pub async fn fetch_node_history() -> Result<NodeHistory, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/node/history"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<NodeHistory>().await.map_err(|e| format!("{e}"))
}

/// Run one job now.
///
/// The backend answers 404 for an unknown id and 500 when the job threw; both
/// carry a `message` explaining which, because "the run failed" and "there is
/// no such job" send the reader to different places.
/// The run list, narrowed by whatever the panel is asking.
///
/// Filtered on the backend rather than here. The record is capped so filtering
/// in the page would work today — and stop working exactly when it starts to
/// matter, which is the point at which there are enough runs for the question
/// to be worth asking.
///
/// Empty strings mean "no filter" rather than "match empty", so the caller can
/// pass a select's value straight through.
pub async fn fetch_runs(
    job: &str,
    outcome: &str,
    since_ms: Option<f64>,
    limit: u32,
) -> Result<RunsResponse, String> {
    let mut url = format!("{API_BASE}/api/runs?limit={limit}");
    if !job.is_empty() {
        url.push_str(&format!("&job={job}"));
    }
    if !outcome.is_empty() {
        url.push_str(&format!("&outcome={outcome}"));
    }
    if let Some(since) = since_ms {
        url.push_str(&format!("&since={}", since as i64));
    }

    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<RunsResponse>().await.map_err(|e| format!("{e}"));
    }
    // A 400 here is a filter the backend refused rather than ignored; showing
    // its message beats showing a list that answers a different question.
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("could not read the run list")
            .to_string()),
        Err(e) => Err(format!("{e}")),
    }
}

/// Run one job now, with what it was asked to do this time.
///
/// `input` is an object keyed by the job's declared field ids — see `JobInput`
/// in the shared crate. Send `{}` for a job that declares none; the backend
/// rejects a body a job did not ask for rather than ignoring it, so an empty
/// object is the right thing to send and not merely the harmless thing.
///
/// A 400 here is the input being wrong, and the job has not started. That
/// distinction is worth keeping: a 500 means it ran and broke.
pub async fn run_job(id: &str, input: &serde_json::Value) -> Result<JobRunResult, String> {
    let resp = gloo_net::http::Request::post(&format!("{API_BASE}/api/jobs/{id}"))
        .header("content-type", "application/json")
        .body(input.to_string())
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<JobRunResult>().await.map_err(|e| format!("{e}"));
    }
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("the run failed")
            .to_string()),
        Err(_) => Err(format!("the run failed ({})", resp.status())),
    }
}

/// Forget one job's memory — its cursors and its window of seen item ids.
///
/// Scoped to one job on purpose. What this replaces was telling people to
/// delete `~/.config/rn/job-state.json`, which is the same act aimed at every
/// job at once: making one report start over also made every other polling job
/// reprocess whatever its source still holds, quietly.
///
/// A 409 means the job is running. Its memory commits when the run ends, so a
/// reset now would be overwritten by writes the caller cannot see — the backend
/// refuses rather than reporting a reset that will not survive the minute.
///
/// Zeroes in the response are an ordinary answer, not a failure: a job that has
/// never run remembers nothing, and `was_empty` is there so the page can say
/// that in words rather than showing "0 cursors".
pub async fn reset_job_state(id: &str) -> Result<StateResetResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/jobs/{id}/state"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<StateResetResponse>().await.map_err(|e| format!("{e}"));
    }
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("the reset failed")
            .to_string()),
        Err(_) => Err(format!("the reset failed ({})", resp.status())),
    }
}

/// Read a job's own source file.
///
/// Takes an id, never a path — the backend resolves the file from the job
/// definition, so there is nothing here a traversal could reach.
pub async fn fetch_job_source(id: &str) -> Result<JobSource, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/jobs/{id}/source"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<JobSource>().await.map_err(|e| format!("{e}"));
    }
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("source unavailable")
            .to_string()),
        Err(_) => Err(format!("source unavailable ({})", resp.status())),
    }
}

/// Read one job's recorded failures.
///
/// Fetched on demand rather than with the job list: no failures is the common
/// case, and sending every job's error messages on every poll would pay for the
/// exception on the ordinary path.
pub async fn fetch_job_errors(id: &str) -> Result<JobErrors, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/jobs/{id}/errors"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<JobErrors>().await.map_err(|e| format!("{e}"));
    }
    Err(format!("could not read the error log ({})", resp.status()))
}
