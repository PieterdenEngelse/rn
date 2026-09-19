//! Talking to the backend: one function per endpoint, plus the diagnosis of
//! what it means when none of them answer.
//!
//! Everything here is `fe`-only and stays here — it depends on `gloo-net` and
//! `web-sys`, neither of which belongs anywhere near a shared type crate.

use super::wire::{
    ConnectionResponse, EnvResponse, HealthResponse, JobConfigResponse, JobErrors, JobOverride,
    JobRunResult, JobSource, JobsResponse, LinksResponse, MailHealthResponse, MailRule,
    PageSaveResponse, PagesResponse, WatchedPage,
    MailRuleSaveResponse, MailRulesResponse, MailTestResponse, MailTestResult, NodeHistory,
    NodeMetrics, RunsDeleteResponse, SendDetail,
    ParamsResponse, RestartOutcome, RunsResponse, SaveResponse, StateResetResponse, StatusResponse,
    CredentialSaveResponse, CredentialsResponse, OAuthDisconnectResponse, OAuthResponse,
    OAuthStartResponse, StopOutcome, TestDelivery, TokenProbe, TokensResponse, WebhookDef,
    WebhookSaveResponse, WebhooksResponse,
};

/// Base URL of the backend API. In development the frontend is served by
/// `dx serve` and the API is a separate process, so this is absolute.
/// A packaged install serves both from one origin — see docs/packaging.md.
///
/// Compiled in, because a wasm bundle has no environment to read at runtime:
/// `fe/serve.sh` sets `RN_API_BASE` from the pane's port so a worktree serving
/// itself talks to its own backend rather than to whichever one happens to
/// hold :3010. `scripts/package.sh` sets it to the empty string, so a packaged
/// page asks for `/api/...` on the origin it was loaded from, which is the
/// backend itself (be/src/web.ts). Unset, as under a bare `dx serve`, it stays
/// the literal every doc names.
pub const API_BASE: &str = match option_env!("RN_API_BASE") {
    Some(base) => base,
    None => "http://127.0.0.1:3010",
};

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

/// Change one runtime setting and leave every other one alone.
///
/// `PUT /api/settings` saves the body **as the whole file**, so posting one
/// field deletes the rest — Config → Runtime avoids that by sending a draft
/// seeded from the server, but a control that saves on the spot has no draft to
/// send. So this reads the saved settings back first, changes the one field,
/// and writes them all.
///
/// Shared rather than copied: the dry-run switch on Monitor → Jobs and every
/// editable row on Config → Jobs need the same three steps, and the failure
/// they prevent is silent — the settings that vanish are the ones nobody was
/// looking at.
pub async fn save_one_setting(id: &str, value: serde_json::Value) -> Result<(), String> {
    let saved = fetch_params().await?.settings;
    let mut map = saved
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);
    map.insert(id.to_string(), value);
    let resp = save_settings(serde_json::Value::Object(map)).await?;
    if resp.ok {
        Ok(())
    } else {
        Err(resp
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("; "))
    }
}

/// Change what this install has overridden about one job.
///
/// The whole override object, not the field that moved: `{ kind: "inherit" }`
/// is how a field goes back to what the job file says, and "send me only what
/// changed" cannot express that — it is the same shape of gap that makes a
/// plain optional field unable to hold both "inherit" and "none".
pub async fn save_job_config(
    id: &str,
    override_: &JobOverride,
) -> Result<JobConfigResponse, String> {
    let body = serde_json::to_string(override_).map_err(|e| format!("{e}"))?;
    // Interpolated like every other per-job endpoint here: ids come from the
    // catalogue rather than from a user, and the backend decodes the segment
    // before looking one up.
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/jobs/{id}/config"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    resp.json::<JobConfigResponse>()
        .await
        .map_err(|e| format!("{e}"))
}

pub async fn fetch_jobs() -> Result<JobsResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/jobs"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<JobsResponse>().await.map_err(|e| format!("{e}"))
}

/// Every send the tracker has minted links for, newest first.
///
/// Read from the API and never from the tracker's own port: that port serves
/// exactly one route to the public internet, and a read endpoint there would be
/// a second thing a stranger can reach.
pub async fn fetch_links() -> Result<LinksResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/links"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<LinksResponse>().await.map_err(|e| format!("{e}"))
}

/// One send's links, with every arrival on each.
///
/// The id is interpolated as the other id-taking calls here do. Send ids are
/// minted by rn rather than typed by anyone, so there is nothing in one that
/// needs escaping — and the backend resolves it against the store, so an id
/// that is not a send is a 404 rather than a path.
pub async fn fetch_send(id: &str) -> Result<SendDetail, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/links/{id}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<SendDetail>().await.map_err(|e| format!("{e}"))
}

/// What is listening, who may talk to it, and what it may reach.
/// Ask a provider whether one credential still works.
///
/// A POST because it is an outward call somebody chose to make. The board
/// itself is derived and free; this is the one thing on it that costs a
/// request, so it happens on a click and never on a poll.
pub async fn probe_token(name: &str) -> Result<TokenProbe, String> {
    let resp = gloo_net::http::Request::post(&format!("{API_BASE}/api/tokens/{name}/probe"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.ok() {
        return Err(format!("backend returned {}", resp.status()));
    }
    resp.json::<TokenProbe>().await.map_err(|e| format!("{e}"))
}

/// When each declared credential stops working, and what stops with it.
///
/// Derived entirely inside the backend from the token's own `exp` claim and
/// from run history, so polling this costs no request against any provider —
/// see `be/src/tokens.ts`.
pub async fn fetch_tokens() -> Result<TokensResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/tokens"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<TokensResponse>()
        .await
        .map_err(|e| format!("{e}"))
}

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

/// Remove every run a filter matches, for one job or for all of them.
///
/// The counts come back rather than a bare ok, because the page's whole claim
/// is "these N", and a delete that removed a different number than the button
/// promised is worth seeing rather than assuming.
pub async fn delete_runs(
    job: &str,
    outcome: &str,
    since_ms: Option<f64>,
) -> Result<RunsDeleteResponse, String> {
    let mut url = format!("{API_BASE}/api/runs?");
    if !job.is_empty() {
        url.push_str(&format!("&job={job}"));
    }
    if !outcome.is_empty() {
        url.push_str(&format!("&outcome={outcome}"));
    }
    if let Some(since) = since_ms {
        url.push_str(&format!("&since={}", since as i64));
    }

    let resp = gloo_net::http::Request::delete(&url)
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.ok() {
        return Err(format!("the runs were not deleted ({})", resp.status()));
    }
    resp.json::<RunsDeleteResponse>().await.map_err(|e| format!("{e}"))
}

/// The same, for a set of jobs — the scoped log's delete.
///
/// One request per job, summed, for the reason [`fetch_runs_for`] makes one
/// per job: the endpoint filters on a single id, and a set of ids is not a
/// thing it needs to learn for two callers to work.
pub async fn delete_runs_for(
    jobs: &[String],
    outcome: &str,
    since_ms: Option<f64>,
) -> Result<RunsDeleteResponse, String> {
    match jobs {
        [] => delete_runs("", outcome, since_ms).await,
        [one] => delete_runs(one, outcome, since_ms).await,
        many => {
            let mut total = RunsDeleteResponse { runs: 0, failures: 0 };
            for id in many {
                let r = delete_runs(id, outcome, since_ms).await?;
                total.runs += r.runs;
                total.failures += r.failures;
            }
            Ok(total)
        }
    }
}

/// The most recent runs of several jobs at once.
///
/// `/api/runs` filters on a single job id, and asking it for two is not a
/// limitation worth widening the endpoint for: any run in the most recent N of
/// a set is necessarily in the most recent N of its own job, so N per job,
/// merged and cut back to N, is exactly the same list the endpoint would
/// return if it took a set.
///
/// `matched` adds up, because the two filters cannot match the same run.
/// `retained` does not: it is how much record exists at all, the same figure
/// in every response, so summing it would report a store several times its own
/// size.
pub async fn fetch_runs_for(
    jobs: &[String],
    outcome: &str,
    since_ms: Option<f64>,
    limit: u32,
) -> Result<RunsResponse, String> {
    match jobs {
        [] => fetch_runs("", outcome, since_ms, limit).await,
        [one] => fetch_runs(one, outcome, since_ms, limit).await,
        many => {
            let mut runs = Vec::new();
            let mut matched = 0;
            let mut retained = 0;
            for id in many {
                let r = fetch_runs(id, outcome, since_ms, limit).await?;
                runs.extend(r.runs);
                matched += r.matched;
                retained = retained.max(r.retained);
            }
            runs.sort_by(|a, b| {
                b.started_at.partial_cmp(&a.started_at).unwrap_or(std::cmp::Ordering::Equal)
            });
            runs.truncate(limit as usize);
            Ok(RunsResponse { runs, matched, retained })
        }
    }
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

/// Send this job a webhook from the backend, over the hooks socket.
///
/// The only way to check a webhook without a provider: pressing Run skips the
/// port, the credential and the signature, which is the half that breaks. A
/// refusal comes back as an ordinary answer with `accepted: false` and a
/// sentence saying which check failed — the listener tells a stranger nothing,
/// and this call is not a stranger.
///
/// It really runs the job. A job that acts on what it receives will act on this
/// payload, which says `rn: "test-delivery"` so it can tell.
pub async fn send_test_delivery(id: &str) -> Result<TestDelivery, String> {
    let resp = gloo_net::http::Request::post(&format!("{API_BASE}/api/jobs/{id}/test-delivery"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    if resp.ok() {
        return resp.json::<TestDelivery>().await.map_err(|e| format!("{e}"));
    }
    match resp.json::<serde_json::Value>().await {
        Ok(v) => Err(v
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("the test delivery could not be sent")
            .to_string()),
        Err(_) => Err(format!("the test delivery could not be sent ({})", resp.status())),
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

/// The webhooks made on Config → Jobs, and what a new one may be made of.
///
/// One request rather than three: the job ids a routing control may offer, the
/// defaults an unfilled field inherits, and whether the listener is actually up
/// are all things the form needs before it can be filled in correctly, and
/// fetching them separately is how a page ends up rendering a dropdown of jobs
/// that no longer exist.
pub async fn fetch_webhooks() -> Result<WebhooksResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/webhooks"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<WebhooksResponse>().await.map_err(|e| format!("{e}"))
}

/// Make or replace one webhook.
///
/// `replacing` is the id it currently has, when this is an edit — the path
/// carries it and the body carries what it becomes, so a rename is one request
/// rather than a delete and a create. That matters more here than it looks: the
/// two-request version leaves the URL answering 404 in between, on an endpoint
/// somebody else's system may be calling.
///
/// A refusal comes back as a list of sentences rather than one message. A form
/// gets several things wrong at once, and fixing them one round-trip at a time
/// is how a person gives up on a page.
pub async fn save_webhook(
    replacing: Option<&str>,
    def: &WebhookDef,
) -> Result<WebhookSaveResponse, String> {
    let id = replacing.unwrap_or(&def.id);
    let body = serde_json::to_string(def).map_err(|e| format!("{e}"))?;
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/webhooks/{id}"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;

    // A 422 is a refused definition, and its body is the useful part — so it is
    // parsed rather than turned into a status code the form cannot act on.
    resp.json::<WebhookSaveResponse>()
        .await
        .map_err(|_| format!("the save failed ({})", resp.status()))
}

/// Remove one webhook. The URL stops answering as soon as this returns.
pub async fn delete_webhook(id: &str) -> Result<WebhookSaveResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/webhooks/{id}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<WebhookSaveResponse>()
        .await
        .map_err(|_| format!("the delete failed ({})", resp.status()))
}

/// What this install needs by way of credentials, and whether it has it.
///
/// Names, variables and two booleans. There is no function in this module that
/// reads a credential's value, and there is no shape in `shared/` that could
/// carry one — see `docs/token-sec.md`, which is the reason rather than a
/// footnote to it: rendering a secret hands it to every local process that can
/// open the port, and to every screenshot of the page.
pub async fn fetch_credentials() -> Result<CredentialsResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/credentials"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<CredentialsResponse>().await.map_err(|e| format!("{e}"))
}

/// Set one credential. Write-only: nothing comes back but whether it took.
///
/// It applies to the running backend immediately — the value goes into the
/// process environment before it is written to the file, which is what arms
/// redaction — so there is no restart to wait for and the page should not
/// suggest one.
///
/// The value is not logged here and must not be put into any error text. An
/// error is written to a console and rendered on a page, which are the two
/// places the whole rule exists to keep a credential out of.
pub async fn save_credential(name: &str, value: &str) -> Result<CredentialSaveResponse, String> {
    let body = serde_json::json!({ "value": value }).to_string();
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/credentials/{name}"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<CredentialSaveResponse>()
        .await
        .map_err(|_| format!("the credential was not saved ({})", resp.status()))
}

/// Remove one, from the running backend and from the file.
///
/// Both halves: leaving the environment would report it as still set, which is
/// true and useless, and leaving the file would bring it back on the next
/// restart.
pub async fn delete_credential(name: &str) -> Result<CredentialSaveResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/credentials/{name}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<CredentialSaveResponse>()
        .await
        .map_err(|_| format!("the credential was not removed ({})", resp.status()))
}

/// The providers rn can sign in to, and what each sign-in left behind.
///
/// Metadata only — who it signed in as, the scopes granted, when it dies. The
/// token itself went into an ordinary credential and never comes back.
pub async fn fetch_oauth() -> Result<OAuthResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/oauth"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.ok() {
        return Err(format!("backend returned {}", resp.status()));
    }
    resp.json::<OAuthResponse>().await.map_err(|e| format!("{e}"))
}

/// Begin a sign-in. Answers the provider URL to send the browser to.
///
/// A POST because it mints single-use state in the backend; a GET would let a
/// link or a prefetch do that.
pub async fn start_oauth(id: &str, scopes: &str) -> Result<OAuthStartResponse, String> {
    let body = serde_json::json!({ "scopes": scopes }).to_string();
    let resp = gloo_net::http::Request::post(&format!("{API_BASE}/api/oauth/{id}/start"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<OAuthStartResponse>()
        .await
        .map_err(|_| format!("the sign-in could not be started ({})", resp.status()))
}

/// Forget the token here, and ask the provider to revoke it.
pub async fn disconnect_oauth(id: &str) -> Result<OAuthDisconnectResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/oauth/{id}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<OAuthDisconnectResponse>()
        .await
        .map_err(|_| format!("the disconnect failed ({})", resp.status()))
}

/// Open both mail connections, authenticate, and close them again.
///
/// POST because it acts on somebody else's server, though it changes nothing
/// here: two connections and two authentications is not a thing a browser, a
/// preview or a link checker should be able to start by fetching a URL.
pub async fn test_mail() -> Result<MailTestResponse, String> {
    let resp = gloo_net::http::Request::post(&format!("{API_BASE}/api/mail-test"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    if !resp.ok() {
        return Err(format!("the test could not be run ({})", resp.status()));
    }
    resp.json::<MailTestResponse>().await.map_err(|e| format!("{e}"))
}

/// What the two mail connections are doing, for Monitor → Mail.
///
/// A different call from [`fetch_mail_rules`] rather than more fields on it,
/// because the two pages ask different questions of the same account: that one
/// shows the rules a person edits, this one the state of two servers, and the
/// sending half has no rules to show.
pub async fn fetch_mail_health() -> Result<MailHealthResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/mail-health"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<MailHealthResponse>().await.map_err(|e| format!("{e}"))
}

/// The mail rules this install has made, and what is actually being watched.
///
/// One call rather than two, because the page's whole job is to show a rule
/// beside whether its mailbox has a connection — and fetching those separately
/// would let them disagree on screen for a moment, which on this page reads as
/// a rule being broken.
/// The pages this install watches, and where the file is.
pub async fn fetch_pages() -> Result<PagesResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/pages"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<PagesResponse>().await.map_err(|e| format!("{e}"))
}

/// Add or replace one watched page. The id in the body decides which; an empty
/// one is minted by the backend, so the page never invents an identity.
pub async fn save_page(page: &WatchedPage) -> Result<PageSaveResponse, String> {
    let body = serde_json::to_string(page).map_err(|e| format!("{e}"))?;
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/pages"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<PageSaveResponse>()
        .await
        .map_err(|_| format!("the save failed ({})", resp.status()))
}

/// Stop watching a page, and forget where it stood.
pub async fn delete_page(id: &str) -> Result<PageSaveResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/pages/{id}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<PageSaveResponse>()
        .await
        .map_err(|_| format!("the delete failed ({})", resp.status()))
}

pub async fn fetch_mail_rules() -> Result<MailRulesResponse, String> {
    let resp = gloo_net::http::Request::get(&format!("{API_BASE}/api/mail-rules"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<MailRulesResponse>().await.map_err(|e| format!("{e}"))
}

/// Add or replace one rule. The id in the body decides which.
pub async fn save_mail_rule(rule: &MailRule) -> Result<MailRuleSaveResponse, String> {
    let body = serde_json::to_string(rule).map_err(|e| format!("{e}"))?;
    let resp = gloo_net::http::Request::put(&format!("{API_BASE}/api/mail-rules"))
        .header("content-type", "application/json")
        .body(body)
        .map_err(|e| format!("{e}"))?
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<MailRuleSaveResponse>()
        .await
        .map_err(|_| format!("the save failed ({})", resp.status()))
}

pub async fn delete_mail_rule(id: &str) -> Result<MailRuleSaveResponse, String> {
    let resp = gloo_net::http::Request::delete(&format!("{API_BASE}/api/mail-rules/{id}"))
        .send()
        .await
        .map_err(|e| format!("{e}"))?;
    resp.json::<MailRuleSaveResponse>()
        .await
        .map_err(|_| format!("the delete failed ({})", resp.status()))
}
