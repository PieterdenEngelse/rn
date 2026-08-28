/**
 * What a job is.
 *
 * A job is a value that describes itself, not a bare function. The difference
 * matters because the frontend has to explain it to someone: a function can be
 * called, but it cannot say what it does, what it would change, or why it was
 * skipped. Everything the Jobs page shows comes from this shape.
 *
 * See docs/jobs.md for the categories of work this is meant to carry, and
 * `run.ts` for the one function allowed to execute any of it.
 */

/** Handed to a job rather than read from a global, so it cannot be missed. */
export interface JobContext {
    /**
     * When true the job must make no change. It should still do all the
     * reading and all the deciding, and report what it *would* have done —
     * a dry run that reports nothing has not proved anything.
     */
    dryRun: boolean;

    /**
     * Structured logging, namespaced to this job. Same rule as log.ts: facts,
     * not prose. `ctx.step("scanned", { files: 412 })`, never
     * `ctx.step("scanning files")`.
     *
     * Each call lands on the run record as well as on stdout, and the Jobs page
     * renders the trace under the run — so this is what the error log shows
     * instead of only a message. The runner keeps the first and last fifty and
     * says how many it dropped; see STEP_HEAD in run.ts.
     */
    step(name: string, detail?: Record<string, unknown>): void;

    /**
     * Aborted when the job passes its timeout. Pass it to `fetch`, to
     * `fs.promises` calls that accept one, and to anything else cancellable.
     *
     * This matters more than it looks. A JavaScript promise cannot be killed
     * from outside — the runner can stop *waiting* for a hung job, but the work
     * itself carries on holding whatever it holds until the process restarts.
     * The signal is the only way work actually stops, and it only works if the
     * job passes it on.
     */
    signal: AbortSignal;

    /**
     * The failure this run is answering, when it is running as another job's
     * `onFailure` handler. Absent on every ordinary run.
     *
     * It is the whole recorded run — the error, the duration, and the steps
     * that ran before it broke — so a handler can report *what* went wrong
     * rather than only that something did. A handler that ignores it is a
     * handler that could not have said which job it was about.
     */
    cause?: JobRun;

    /**
     * What this run was asked to do, with the job's declared defaults already
     * filled in — so a job reads `ctx.input.maxAgeDays` without checking
     * whether anyone supplied it.
     *
     * Empty for a job that declares no inputs. Values are checked against the
     * declared type before `run()` is called, so a cast here is safe in the way
     * a cast on a request body is not; there is no per-job generic because the
     * runner handles every job through one signature.
     */
    input: Record<string, JsonValue>;

    /**
     * What this job remembers from its last successful run.
     *
     * The answer to "have I seen this already?", and the only one there is: a
     * job process starts with nothing, and the run record is a log rather than
     * a place to look things up. A poller without this reports every item on
     * every run — noise a person learns to scroll past, which is the same as
     * reporting nothing.
     *
     * **Writes are staged and committed by the runner, not by this call.** They
     * land only if the run finishes without throwing, and never under dry run.
     * That is deliberately not the job's decision: a cursor advanced by a run
     * that then failed skips forever the items it had read and not acted on,
     * and nothing about the failed record says anything was lost. See
     * `state.ts`, which carries the whole rule.
     *
     * Values are scrubbed of every configured secret before they reach disk,
     * exactly like the summary and the step details.
     */
    state: JobState;

    /**
     * A credential, by name — never by value.
     *
     * Throws if the job did not declare the name in `credentials`, so the
     * declaration cannot drift from the use: a job that quietly reads a
     * credential it never declared is one the Jobs page cannot warn you about
     * when it is missing.
     *
     * The value is never logged by the runner, and every configured secret is
     * scrubbed out of step details, summaries, inputs and error messages before
     * anything is written or shown — see `be/src/secrets.ts`. That protects the
     * record from a job that reports its own token by accident. It cannot
     * protect against a job that sends it somewhere; nothing can.
     */
    secret(name: string): string;

    /**
     * The body of the webhook delivery that started this run, parsed.
     *
     * Absent on every other run, exactly like `cause` — a field set by one
     * trigger kind, so a job can tell which door it came in by rather than
     * being told which mode it is in.
     *
     * It is **not** `input`, and the difference is the point. `input` is
     * declared, typed and checked against the declaration before anything
     * starts; a provider's payload is arbitrary nested JSON that no job can
     * declare in advance. Routing it through `resolveInput` would mean
     * loosening a check that exists so a typo in a field name cannot run a job
     * with defaults and report success. So the payload arrives beside the
     * input, and a webhook job reads both: defaults from its declaration, facts
     * from the delivery.
     *
     * The signature was verified before the job was started, so this came from
     * the holder of the secret — which is a different statement from "this is
     * well-formed". Read it defensively; the provider changed their schema
     * without telling you.
     */
    payload?: JsonValue;
}

/**
 * The shapes that cross a boundary come from the shared crate, not from here.
 *
 * `JobResult`, `JobInfo` and `Schedule` are all sent to the frontend, so they
 * are defined once in `shared/src/jobs.rs` and regenerated into
 * `be/src/generated/wire.ts`. Re-exported rather than merely imported, so a
 * job file keeps importing everything it needs from one place.
 *
 * `Job` and `JobContext` below stay local: they carry `run()` and a `step()`
 * callback, which are behaviour and cannot cross a process boundary at all.
 */
export type { JobResult, JobInfo, JobInput, Schedule } from "../generated/wire.ts";
import type { JobInfo, JobInput, JobRun, Schedule } from "../generated/wire.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";
import type { JobState } from "./state.ts";
import type { JobResult } from "../generated/wire.ts";

export interface Job {
    /** Stable identifier. Appears in the URL, in logs, and in the registry. */
    id: string;
    /** Short human name for the Jobs page. */
    label: string;
    info: JobInfo;
    /**
     * Absent for a job that only ever runs when asked. A scheduled job is still
     * runnable by hand — the schedule is an extra door, not a replacement.
     */
    schedule?: Schedule;

    /**
     * Absolute path to the file this job is defined in. Set it to
     * `import.meta.filename` and it cannot drift: a mapping kept anywhere else
     * is one a rename silently invalidates.
     *
     * Exists so the Jobs page can show the job's own source. An info panel says
     * what a job does in prose; the source says what it actually does, which is
     * the version that is true. It is also what makes the path from "a control
     * on a page" to "the code behind it" a click rather than a search.
     *
     * The request never carries a path — only a job id, looked up in the
     * catalogue — so there is nothing here for a traversal to reach.
     */
    source: string;

    /**
     * Wall-clock ceiling for one run, in milliseconds. Defaults to
     * DEFAULT_TIMEOUT_MS in run.ts.
     *
     * There is no way to opt out, and that is deliberate: a job with no ceiling
     * that hangs takes the whole app with it quietly. It never finishes, so the
     * in-flight registry never empties, so every future restart queues behind
     * it forever and the scheduler skips its slot every night as "still
     * running". Nothing has failed, so nothing is red. Set a large number for a
     * job that genuinely runs for hours — that is a statement about the job,
     * which is worth having on the record.
     */
    timeoutMs?: number;

    /**
     * The id of another job to run when this one fails.
     *
     * The answer to "nothing pushes" that does not require building a
     * notification system: instead of SMTP settings and a template, you write a
     * job, and that job can do whatever you want. It runs through `runJob` like
     * anything else, so it is tracked, timed and recorded — a failure produces
     * two run records, and the second says `trigger: "failure"` and carries
     * `causedBy` so the pair reads as one story.
     *
     * The handler is given the failed run as `ctx.cause`.
     *
     * **One hop only.** A handler's own failure starts nothing, so a job that
     * names itself, or a pair that name each other, terminates rather than
     * recursing. The refusal is logged, never swallowed — see `runJob`.
     */
    onFailure?: string;

    /**
     * The id of another job to run when this one changes something.
     *
     * The counterpart of `onFailure`, and the answer to "nothing pushes" for
     * the case that is not a failure. A job that reports `changed: true` hands
     * off to this one, which can post to a webhook, write a file, or whatever
     * else you want — so "tell me when something moved" is a job you write
     * rather than a notification system rn has to grow, SMTP settings and all.
     *
     * It matters more than the failure path, not less. A job that fails is
     * visible: the header light goes amber, the run is red, the error log
     * fills. A job that quietly succeeds at noticing something and tells nobody
     * looks exactly like a job that is working, and `watch-upstreams` — which
     * fires at 04:00 and writes its report to a page somebody has to open — is
     * why this exists.
     *
     * **`changed`, not "ran".** A run that found nothing to do, or was skipped,
     * or was disarmed by dry run, starts nothing: those are all `changed:
     * false`, and a handler that fired on every run would be a daily message
     * saying nothing happened, which is the thing people mute. That also means
     * dry run disables this path in passing, because a dry run reports
     * `changed: false` by construction.
     *
     * The handler is given the changed run as `ctx.cause`, exactly as a failure
     * handler is given the failed one — so it can say *what* changed rather
     * than only that something did.
     *
     * **One hop only**, and the same rule as `onFailure`: a handler's own
     * change starts nothing, so a job that names itself, or a pair that name
     * each other, terminates rather than recursing. The refusal is logged.
     */
    onChange?: string;

    /**
     * Try again when this job fails: how many attempts in total, and the fixed
     * wait between them.
     *
     * `attempts` counts runs, not extra runs — `3` is one attempt and two more.
     * Absent means one attempt, which is not the same statement as
     * `attempts: 1`.
     *
     * The ceiling in `timeoutMs` is **per attempt**, so three attempts of a
     * five-minute job can occupy fifteen minutes plus the waits. That is the
     * less surprising reading of a per-job timeout, but it is worth knowing
     * before setting both large.
     *
     * **A timed-out attempt is retried only if the work actually stopped.** A
     * promise cannot be cancelled from outside, so an attempt that hit its
     * ceiling is still running unless the job honoured `ctx.signal` — and
     * starting a second copy against the same files would be worse than the
     * failure. The runner waits `ABORT_GRACE_MS` for the work to settle and
     * gives up the retry if it does not, recording `retry-abandoned` in the
     * trace rather than silently running once. Passing `ctx.signal` on is what
     * makes retries work for slow jobs.
     *
     * **A failure the job knows is permanent is not retried.** Throwing
     * `PermanentFailure` from `./permanent.ts` — a 4xx from a receiver, a
     * malformed input, a runtime permission the process cannot be granted while
     * it runs — fails the run on the spot and writes `retry-skipped` into the
     * trace with the reason. The policy stays declared and still covers every
     * other failure in the same job: it marks one error, not one job.
     *
     * One run record covers the whole sequence, carrying `attempts`; the errors
     * the earlier attempts hit are in `steps` as `retry` entries.
     */
    retry?: { attempts: number; backoffMs: number };

    /**
     * Accept a webhook: which credential signs it, and where the signature is.
     *
     * Declaring this is what puts the job behind `POST /api/hooks/:id` on the
     * hooks listener. A job without it is not reachable there at all, and the
     * listener answers 404 rather than 403 — an unconfigured id must not be
     * distinguishable from one that does not exist, or the endpoint becomes a
     * way to enumerate the catalogue.
     *
     * **There is no unsigned mode.** The listener is reachable from the
     * internet through a tunnel, and its URL is a bearer capability: anyone who
     * learns it can post to it. The signature is the only thing standing
     * between that and a stranger running your automations, so `credential` is
     * required rather than optional.
     *
     * The secret is an ordinary credential — `RN_SECRET_<NAME>` in
     * `~/.config/rn/credentials`, read by the launcher, redacted from every
     * record. No second store, and the same rules as a token a job sends.
     *
     * Defaults are GitHub's, because it is the most common sender and its
     * scheme — HMAC-SHA256 hex under `X-Hub-Signature-256`, prefixed
     * `sha256=` — is also what Slack and most others use under a different
     * header name.
     */
    webhook?: {
        /** Credential name the signature is verified against. Required. */
        credential: string;
        /** Header carrying the signature. Lowercased on read. */
        header?: string;
        /** Prefix on the header value, `""` for a bare hex digest. */
        prefix?: string;
        /**
         * Header carrying a unique delivery id, if the provider sends one.
         * Without it a captured request can be replayed — a signature stays
         * valid forever, which is what a signature is — so set it wherever the
         * provider offers one. GitHub: `x-github-delivery`.
         */
        deliveryHeader?: string;
        /**
         * Header naming what happened, in the provider's vocabulary — "push",
         * "pull_request", "invoice.paid". Recorded on the run so the history
         * says which kind of delivery it answered rather than forty identical
         * rows. GitHub: `x-github-event`, which is the default.
         */
        eventHeader?: string;
    };

    /**
     * Values this job accepts for a single run, rendered as a form on the Jobs
     * page and checked before anything starts.
     *
     * The input belongs to the run rather than to the install, which is the
     * whole distinction from a runtime parameter: "process this folder" is
     * said once, not saved. What was actually used is recorded on the run, or
     * two runs with different inputs would be indistinguishable in the history.
     *
     * A field with no `default` is required. **A scheduled job must give every
     * input a default**, because the scheduler supplies none — there is a test
     * for it, since "scheduled" quietly meaning "runs with undefined
     * everywhere" is exactly the failure that would not announce itself.
     */
    inputs?: JobInput[];

    /**
     * Names of the credentials this job needs, resolved through `ctx.secret`.
     *
     * Declared rather than merely used, so the Jobs page can say which are
     * missing *before* a run — and so the runner can refuse to start a job
     * whose credential is absent, rather than letting it send an empty header
     * and fail somewhere less legible. All declared credentials are required;
     * a job that can work without one should not declare it.
     *
     * Values live outside the codebase. Today that is the environment, one
     * variable per name (`githubToken` → `RN_SECRET_GITHUB_TOKEN`); see
     * `be/src/secrets.ts` for why the indirection exists rather than jobs
     * reading `process.env` directly.
     */
    credentials?: string[];
    run(ctx: JobContext): Promise<JobResult>;
}
