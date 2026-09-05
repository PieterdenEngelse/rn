/**
 * The API the frontend reads. Deliberately small: node:http, no framework.
 *
 * GET  /api/health    liveness
 * GET  /api/params    the parameter registry + live values + saved settings
 * PUT  /api/settings  save settings (validated); reports what needs a restart
 * GET  /api/jobs      the catalogue, what is running, scheduled, and what ran
 * POST /api/jobs/:id  run one job now
 * GET  /api/jobs/:id/source   the job's own source file
 * GET  /api/jobs/:id/errors   the job's recorded failures
 * GET  /api/connection  what is listening, who may talk to it, what it may reach
 * GET  /api/env       what be/.env says, against what this process has
 * GET  /api/credentials    what this install needs, and whether it has it
 * PUT  /api/credentials/:name    set one (write-only; never read back)
 * DELETE /api/credentials/:name  remove one
 * GET  /api/webhooks  the webhooks made on the page, and what may be chosen for one
 * PUT  /api/webhooks/:id     make or replace one
 * DELETE /api/webhooks/:id   remove one
 *
 * The hooks listener is deliberately not here. It is a separate server on its
 * own port with one route — see be/src/hooks/server.ts.
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { RUNTIME_PARAMS, WITHHELD } from "./runtime-params.ts";
// The response shapes are defined in `shared/src/params.rs`, so `satisfies`
// below is what makes a field renamed there a build failure here rather than an
// `undefined` in whichever panel reads it first.
import type {
    CredentialSaveResponse,
    CredentialsResponse,
    WebhookDef,
    WebhookSaveResponse,
    WebhooksResponse,
    ParamsResponse,
    RestartOutcome,
    SaveResponse,
    StateResetResponse,
    StatusResponse,
    StopOutcome,
} from "./generated/wire.ts";
import {
    applyRuntimeSettings,
    isSupervised,
    pendingRestart,
    effectiveValues,
    load,
    save,
    validateAll,
    needsRestart,
    type Settings,
} from "./settings.ts";
import { config, remoteBindRefusal } from "./config.ts";
import { createHookApp, hooksHealth, startHooks } from "./hooks/server.ts";
import { sendTestDelivery } from "./hooks/test-delivery.ts";
import { describeEnv } from "./env-file.ts";
import * as webhooks from "./webhooks.ts";
import * as credentialsFile from "./credentials-file.ts";
import { dryRun } from "./dry-run.ts";
import * as secrets from "./secrets.ts";
import { display as displayPath } from "./paths.ts";

/**
 * Exit code that asks the launcher for a restart. Must match EXIT_RESTART in
 * launcher/src/lib.rs — the two halves of one protocol.
 */
const EXIT_RESTART = 75;

/**
 * Exit code that tells the launcher not to restart us. Must match EXIT_FATAL in
 * launcher/src/lib.rs — the other half of the same protocol.
 *
 * Used for a failure a restart cannot fix. The API's port being occupied is the
 * whole of that category in practice: retrying binds the same address to the
 * same taken port, five times, half a second apart.
 */
const EXIT_FATAL = 78;
import { step, debug, error } from "./log.ts";
import * as running from "./running.ts";
import {
    JOBS,
    jobById,
    resolveInput,
    runJob,
    scheduler,
    history as jobHistory,
    state as jobState,
    defaultTimeoutMs,
} from "./jobs/index.ts";
import { collect as collectNodeMetrics, lifetimeDelay } from "./node_metrics.ts";
import { withDistribution } from "./node_history.ts";

function send(res: ServerResponse, code: number, body: unknown): void {
    const json = JSON.stringify(body);
    res.writeHead(code, {
        "content-type": "application/json; charset=utf-8",
        "content-length": Buffer.byteLength(json),
        // CORS headers are not set here. They depend on the request's Origin,
        // which this function does not have, and they are set once per request
        // in the handler instead — writeHead merges what setHeader already put
        // on the response.
    });
    res.end(json);
}

async function readJson(req: IncomingMessage): Promise<unknown> {
    const chunks: Buffer[] = [];
    let size = 0;
    for await (const chunk of req) {
        size += (chunk as Buffer).length;
        if (size > 64 * 1024) throw new Error("request body too large");
        chunks.push(chunk as Buffer);
    }
    if (chunks.length === 0) return {};
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

/**
 * Which parts of this install ask for a credential, by name.
 *
 * Assembled from all four places a credential can be declared — a job's own
 * list, a job's webhook, a page-made webhook, and that webhook's lookup — so
 * the board is a list of what this install actually needs rather than a list of
 * what somebody happened to type once.
 *
 * A name nothing declares still appears, from the file, because a credential
 * added ahead of the job that will want it is a reasonable thing to have done
 * and a page that hid it would look like it had lost the value.
 */
function credentialDeclarations(): Map<string, string[]> {
    const out = new Map<string, string[]>();
    const add = (name: string | null | undefined, by: string): void => {
        if (typeof name !== "string" || name === "") return;
        const list = out.get(name) ?? [];
        if (!list.includes(by)) list.push(by);
        out.set(name, list);
    };

    for (const job of JOBS) {
        for (const name of job.credentials ?? []) add(name, job.id);
        add(job.webhook?.credential, job.id);
        add(job.webhook?.auth?.kind === "token" ? job.webhook.credential : undefined, job.id);
    }
    for (const def of webhooks.list()) {
        add(def.credential, def.id);
        add(def.lookup?.credential, def.id);
    }
    return out;
}

/** What declares one credential. Empty when nothing currently does. */
function declaredBy(name: string): string[] {
    return credentialDeclarations().get(name) ?? [];
}

/**
 * Every credential worth a row: everything declared, plus everything the file
 * names, sorted so the board does not reshuffle between polls.
 */
function credentialEntries() {
    const declarations = credentialDeclarations();
    const names = new Set(declarations.keys());

    const rows = [...names]
        .sort((a, b) => a.localeCompare(b))
        .map((name) => credentialsFile.describe(name, declarations.get(name) ?? []));

    // Variables the file sets that nothing declares. Shown under the variable's
    // own spelling: the name→variable mapping is one-way by design, so there is
    // no name to recover, and a guessed one would not match what any job asks
    // for. See describeVariable().
    const covered = new Set(rows.map((r) => r.envVar));
    for (const variable of credentialsFile.fileVars()) {
        if (!covered.has(variable)) rows.push(credentialsFile.describeVariable(variable));
    }

    return rows;
}

/** Set once a restart is queued behind running work. */
let restartWhenIdle = false;

/**
 * Exit with the code the launcher watches for. It rebuilds the environment
 * from the settings as they are now and starts a fresh process.
 */
function doRestart(): void {
    closeListeners(server);
    // Let the HTTP response flush first.
    setTimeout(() => process.exit(EXIT_RESTART), 100);
}

export function createApp() {
    // Async because a job runs inside the request. Every other route is
    // synchronous and unaffected; the one awaiting handler is the POST below.
    return createServer(async (req, res) => {
        const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
        const started = Date.now();

        // The dev frontend runs on a different port (dx serve :1790), so the
        // browser treats it as cross-origin. Dev-only convenience: in a
        // packaged install the launcher serves both from one origin.
        //
        // The header carries exactly one origin — "*" is not an option once
        // credentials or a narrow allowlist are wanted — so the request's own
        // Origin is echoed when it is on the list, and the first entry stands
        // in otherwise. Vary tells caches the answer depends on it.
        const origin = req.headers.origin;
        res.setHeader(
            "access-control-allow-origin",
            origin && config.corsOrigins.includes(origin) ? origin : config.corsOrigins[0]!,
        );
        res.setHeader("vary", "origin");
        // DELETE is on the list because two routes use it — a job's memory and
        // a webhook. It was missing while only the first existed, which a
        // same-origin packaged install never notices and the dev frontend hits
        // as a failed preflight with nothing in the response to say why.
        res.setHeader("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS");
        res.setHeader("access-control-allow-headers", "content-type");

        const done = (code: number): void => {
            debug("http", { method: req.method, path: url.pathname, code, ms: Date.now() - started });
        };

        if (req.method === "OPTIONS") {
            send(res, 204, {});
            return done(204);
        }

        if (url.pathname === "/api/health") {
            // The hooks listener cannot answer for itself — it has one route
            // and is the port a tunnel points at, so it is deliberately not
            // given a GET. Reported from here instead, in the same process,
            // from the socket's own `listening` flag.
            const hooksState = hooksHealth();
            send(res, 200, {
                status: hooksState.error === null ? "ok" : "degraded",
                node: process.version,
                hooks: hooksState,
            });
            return done(200);
        }

        if (url.pathname === "/api/params" && req.method === "GET") {
            const settings = load(config.settingsPath);
            send(res, 200, {
                // Spread rather than passed: the registries are `readonly`
                // arrays, which a mutable `Array<T>` field will not accept.
                params: [...RUNTIME_PARAMS],
                withheld: [...WITHHELD],
                effective: effectiveValues(),
                settings,
                supervised: isSupervised(),
                pending: pendingRestart(settings),
            } satisfies ParamsResponse);
            return done(200);
        }

        if (url.pathname === "/api/node/history" && req.method === "GET") {
            const { percentile, max } = lifetimeDelay();
            send(res, 200, withDistribution(percentile, max));
            return done(200);
        }

        if (url.pathname === "/api/node" && req.method === "GET") {
            send(res, 200, collectNodeMetrics());
            return done(200);
        }

        if (url.pathname === "/api/status" && req.method === "GET") {
            send(res, 200, {
                supervised: isSupervised(),
                pid: process.pid,
                launcherPid: process.env["RN_LAUNCHER_PID"] ?? null,
                uptimeMs: Math.round(process.uptime() * 1000),
                node: process.version,
                execPath: displayPath(process.execPath),
                settingsPath: displayPath(config.settingsPath),
                url: `http://${config.host}:${config.port}`,
                jobs: running.count(),
                restartPending: restartWhenIdle,
                // Lets the header light go amber without a second request.
                pendingCount: pendingRestart(load(config.settingsPath)).length,
                // Jobs whose *most recent* run failed. Most-recent rather than
                // ever-failed so the light clears itself on the next success —
                // a warning that never goes out is one people learn to ignore.
                failedJobs: JOBS.filter((j) => {
                    const last = jobHistory.lastFor(j.id);
                    return last !== undefined && jobHistory.outcome(last) === "failed";
                }).length,
            } satisfies StatusResponse);
            return done(200);
        }

        // What is listening, who may talk to it, and what it may reach. One
        // request rather than three, because the three questions behind
        // "backend unreachable" are always asked together.
        if (url.pathname === "/api/connection" && req.method === "GET") {
            const granted = (process.env["RN_NET_ALLOWLIST"] ?? "")
                .split(",")
                .map((h) => h.trim())
                .filter(Boolean);
            const extra = (process.env["RN_NET_EXTRA"] ?? "")
                .split(",")
                .map((h) => h.trim())
                .filter(Boolean);
            const versions = process.versions as Record<string, string | undefined>;
            const runtime = versions.bun ? "bun" : versions.deno ? "deno" : "node";
            send(res, 200, {
                host: config.host,
                port: config.port,
                url: `http://${config.host}:${config.port}`,
                // Named rather than derived on the page: what counts as
                // loopback is a property of the address, and the frontend
                // guessing at it would be a second implementation to keep
                // right.
                loopbackOnly:
                    config.host === "127.0.0.1" ||
                    config.host === "::1" ||
                    config.host === "localhost",
                corsOrigins: config.corsOrigins,
                // Reported so the Webhooks board can say which port to point a
                // tunnel at — and, by saying it is not `port`, why.
                hooksPort: config.hooksPort,
                webhookJobs: JOBS.filter((j) => j.webhook !== undefined).length,
                webhookReady: JOBS.filter(
                    (j) => j.webhook !== undefined && secrets.isSet(j.webhook.credential),
                ).length,
                // Counted apart from the total because it is a different claim
                // about this install: a token does not cover the body, so one
                // that leaks forges every delivery until it is rotated.
                webhookTokenJobs: JOBS.filter((j) => j.webhook?.auth?.kind === "token").length,
                runtime,
                // What the launcher passed, not what was saved — the two differ
                // until a restart, which is exactly when someone looks here.
                netGranted: granted,
                netExtra: extra,
                // Only Deno checks it. Under Node and Bun the list is recorded
                // and nothing enforces it, and a page that implied otherwise
                // would be describing a guarantee this process does not have.
                netEnforced: runtime === "deno",
                supervised: isSupervised(),
            });
            return done(200);
        }

        // What be/.env says against what this process has. The file is read
        // fresh on every request — reading it once at startup would make the
        // board unable to report the very drift it exists for.
        if (url.pathname === "/api/env" && req.method === "GET") {
            send(res, 200, describeEnv());
            return done(200);
        }

        if (url.pathname === "/api/stop" && req.method === "POST") {
            const runningJobs = running.list();
            const force = url.searchParams.get("force") === "1";
            if (runningJobs.length > 0 && !force) {
                send(res, 409, {
                    ok: false,
                    error: "jobs running",
                    message: "Work is in progress. Stop anyway with force, or wait for it to finish.",
                    running: runningJobs,
                } satisfies StopOutcome);
                return done(409);
            }
            send(res, 200, {
                ok: true,
                message: "stopping",
                aborted: runningJobs,
            } satisfies StopOutcome);
            done(200);
            step("stop-requested", { force, aborting: runningJobs.length });
            closeListeners(server);
            // Exit 0: the launcher treats that as an intentional stop and exits
            // too, rather than restarting us.
            setTimeout(() => process.exit(0), 100);
            return;
        }

        if (url.pathname === "/api/jobs" && req.method === "GET") {
            send(res, 200, {
                running: running.list(),
                restartPending: restartWhenIdle,
                // The catalogue, so the Jobs page can list what exists rather
                // than only what happens to be running at the moment it loads.
                // Carries each job's info-panel prose; see be/src/jobs/types.ts.
                catalogue: JOBS.map((j) => ({
                    id: j.id,
                    label: j.label,
                    info: j.info,
                    // Display form only — the reader needs to know which file
                    // they are about to open. The absolute path is never sent,
                    // and never accepted back.
                    source: displayPath(j.source),
                    // Surfaced because a ceiling nobody can see is a surprise
                    // when it fires. Resolved here rather than sent as
                    // "undefined means the default", so the page never has to
                    // know what the default is.
                    timeoutMs: j.timeoutMs ?? defaultTimeoutMs(),
                    // Same argument as the schedule above: a failure path
                    // nobody can see is indistinguishable from no failure
                    // path, and the user only finds out which they had when
                    // the job fails.
                    ...(j.onFailure === undefined ? {} : { onFailure: j.onFailure }),
                    // And the path that carries the news. Worth surfacing more
                    // than the failure one, not less: an unwired failure path
                    // is invisible and harmless, while a job whose whole point
                    // is to tell you something, wired to nothing, still looks
                    // like a job that is working.
                    ...(j.onChange === undefined ? {} : { onChange: j.onChange }),
                    // Always sent, never conditional: "this job remembers even
                    // while disarmed" is the answer to why one report is
                    // incremental and the next one repeats, and a field that
                    // vanishes when false makes the page infer that from an
                    // absence.
                    effectFree: j.effectFree === true,
                    // Whether a hook is configured and whether its secret is
                    // there — never the secret, and never the URL. A tunnel
                    // address is a bearer capability: anyone holding it can
                    // reach the listener, so it is not a thing to put on a
                    // page or in a payload. See WebhookInfo in
                    // shared/src/jobs.rs.
                    ...(j.webhook === undefined
                        ? {}
                        : {
                              webhook: {
                                  header: (
                                      j.webhook.auth?.header ??
                                      j.webhook.header ??
                                      "x-hub-signature-256"
                                  ).toLowerCase(),
                                  credential: j.webhook.credential,
                                  // The variable to set, named here rather than
                                  // spelled out by the page: the camel-case
                                  // split lives in secrets.ts and a second copy
                                  // of it would be wrong first on the names
                                  // nobody can guess.
                                  envVar: secrets.envVarFor(j.webhook.credential),
                                  secretSet: secrets.isSet(j.webhook.credential),
                                  // Which of the two kinds of proof, and — for
                                  // a signature — which construction. Sent
                                  // because "webhook configured" is not one
                                  // security position but two, and a page that
                                  // spells them the same way hides the weaker.
                                  auth: j.webhook.auth?.kind === "token" ? "token" : "signature",
                                  ...(j.webhook.auth?.kind === "token"
                                      ? {}
                                      : { scheme: j.webhook.scheme ?? "hmac-body" }),
                                  ...(j.webhook.respond === undefined
                                      ? {}
                                      : { respondDeadlineMs: j.webhook.respond.deadlineMs }),
                              },
                          }),
                    // And for the same reason again: three attempts of a
                    // five-minute job is a fifteen-minute worst case, which
                    // nobody can work out from a page that does not say the
                    // job retries at all.
                    ...(j.retry === undefined ? {} : { retry: j.retry }),
                    // The form the Jobs page renders. Sent even though the
                    // backend is the one that validates: a field the user
                    // cannot see is one they cannot supply, and a required
                    // input they do not know about is a 400 with no way to fix
                    // it from the page.
                    inputs: j.inputs ?? [],
                    // Names, variables and whether each is set — never a value,
                    // and never a prefix or a length of one. A job that will
                    // fail at 03:00 for want of a token otherwise looks exactly
                    // like one that will work.
                    credentials: (j.credentials ?? []).map((name) => secrets.describe(name)),
                    // How much this job is holding between runs. Counts only,
                    // for the reason state.stats() gives: a cursor is whatever
                    // a source uses as an identifier, and reporting that
                    // something is remembered is a different act from showing
                    // what.
                    //
                    // Per job rather than as an aggregate, because the row uses
                    // it to decide whether it has anything to offer at all —
                    // "forget this job's memory" on a job with no memory reads
                    // as though it has one.
                    remembered: jobState.statsFor(j.id),
                })),
                dryRun: dryRun(),
                // The knobs every run is subject to, whichever job it is.
                // Resolved here for the same reason `timeoutMs` above is: they
                // are constants in be/src/jobs/, and Config → Jobs reports what
                // this process actually has rather than a copy that drifts.
                config: {
                    defaultTimeoutMs: defaultTimeoutMs(),
                    schedulerTickMs: scheduler.tickMs(),
                    historyCapacity: jobHistory.capacities().runs,
                    failureCapacity: jobHistory.capacities().failures,
                    stateCursorsPerJob: jobState.maxCursors(),
                    stateSeenPerJob: jobState.seenCapacity(),
                    // Counts, never values. What a job remembers is whatever
                    // its source hands out as an identifier, and a page that
                    // showed one would be broadcasting it — see
                    // docs/token-sec.md and stats() in be/src/jobs/state.ts.
                    stateCursors: jobState.stats().cursors,
                    stateJobs: jobState.stats().jobs,
                },
                // What fires on its own, and when next. Shown on the Jobs page
                // because a schedule nobody can see is indistinguishable from
                // no schedule at all — and this one deliberately does not
                // catch up on missed slots, which is only defensible if the
                // next slot is visible.
                scheduled: scheduler.status(),
                // The last run of each job, and a short log of everything.
                // `outcome` is derived here rather than stored, so an older
                // record cannot carry a verdict by a rule that has since
                // changed — see be/src/jobs/history.ts.
                lastRuns: JOBS.map((j) => {
                    const run = jobHistory.lastFor(j.id);
                    return run === undefined
                        ? null
                        : { ...run, outcome: jobHistory.outcome(run) };
                }).filter((r) => r !== null),
                // The run list moved to GET /api/runs when it grew filters.
                // Left here it would be a second, unfiltered copy nobody reads
                // — and 25 full records, steps and inputs included, on every
                // request for something else.
            });
            return done(200);
        }

        // The run list, narrowed. Its own endpoint rather than more fields on
        // GET /api/jobs: that one answers "what exists and what is happening
        // now" and is polled for it, while this one is asked a question and
        // re-asked when the question changes.
        if (url.pathname === "/api/runs" && req.method === "GET") {
            const q = url.searchParams;
            const jobId = q.get("job") ?? undefined;
            const outcome = q.get("outcome") ?? undefined;
            const since = q.get("since") ?? undefined;
            const limit = q.get("limit") ?? undefined;

            // Every bad value is refused rather than ignored. A filter that
            // silently falls back to "everything" shows a list that answers a
            // different question than the one on screen, and nothing about it
            // looks wrong.
            const errors: string[] = [];
            if (jobId !== undefined && !jobById(jobId)) {
                errors.push(`no job with id "${jobId}"`);
            }
            if (outcome !== undefined && !jobHistory.OUTCOMES.includes(outcome as jobHistory.Outcome)) {
                errors.push(`outcome must be one of ${jobHistory.OUTCOMES.join(", ")}`);
            }
            const sinceMs = since === undefined ? undefined : Number(since);
            if (sinceMs !== undefined && !Number.isFinite(sinceMs)) {
                errors.push("since must be epoch milliseconds");
            }
            const limitN = limit === undefined ? undefined : Number(limit);
            if (limitN !== undefined && (!Number.isInteger(limitN) || limitN < 1)) {
                errors.push("limit must be a positive integer");
            }
            if (errors.length > 0) {
                send(res, 400, { message: errors.join("; "), errors });
                return done(400);
            }

            // Spread rather than passed as undefined: exactOptionalPropertyTypes
            // is on, and "absent" and "present but undefined" are the same
            // question asked twice.
            const result = jobHistory.query({
                ...(jobId === undefined ? {} : { jobId }),
                // Checked against OUTCOMES above, which is what makes this safe.
                ...(outcome === undefined ? {} : { outcome: outcome as jobHistory.Outcome }),
                ...(sinceMs === undefined ? {} : { since: sinceMs }),
                ...(limitN === undefined ? {} : { limit: limitN }),
            });
            send(res, 200, {
                // Derived here rather than stored, like everywhere else it is
                // sent — see be/src/jobs/history.ts.
                runs: result.runs.map((run) => ({ ...run, outcome: jobHistory.outcome(run) })),
                matched: result.matched,
                retained: result.retained,
            });
            return done(200);
        }

        // A job's own source. The path is taken from the job definition, not
        // from the request — the URL carries an id that must match a job in
        // the catalogue — so there is no path here for a traversal to reach.
        if (url.pathname.startsWith("/api/jobs/") && url.pathname.endsWith("/source")
            && req.method === "GET") {
            const id = decodeURIComponent(
                url.pathname.slice("/api/jobs/".length, -"/source".length),
            );
            const job = jobById(id);
            if (!job) {
                send(res, 404, { message: `No job with id "${id}".` });
                return done(404);
            }
            try {
                const content = await readFile(job.source, "utf8");
                send(res, 200, { id: job.id, path: displayPath(job.source), content });
                return done(200);
            } catch (err) {
                // A packaged install could conceivably ship without sources.
                // Say which file was missing rather than returning an empty box.
                send(res, 404, {
                    message: `Could not read ${displayPath(job.source)}: ${
                        err instanceof Error ? err.message : String(err)
                    }`,
                });
                return done(404);
            }
        }

        // One job's failures. Separate from GET /api/jobs so the page can open
        // it on demand: a job with no failures is the common case, and sending
        // fifty stack-trace-length messages for every job on every poll would
        // be paying for the exception on the ordinary path.
        if (url.pathname.startsWith("/api/jobs/") && url.pathname.endsWith("/errors")
            && req.method === "GET") {
            const id = decodeURIComponent(
                url.pathname.slice("/api/jobs/".length, -"/errors".length),
            );
            const job = jobById(id);
            if (!job) {
                send(res, 404, { message: `No job with id "${id}".` });
                return done(404);
            }
            const counts = jobHistory.countsFor(job.id);
            send(res, 200, {
                id: job.id,
                failures: jobHistory
                    .failuresFor(job.id)
                    .map((run) => ({ ...run, outcome: jobHistory.outcome(run) })),
                runsRetained: counts.runs,
                failuresRetained: counts.failures,
            });
            return done(200);
        }

        // Send this job a webhook, from here, over the socket.
        //
        // The one trigger that cannot be checked by pressing Run: running a job
        // by hand skips the port, the credential and the signature, which is
        // the half that goes wrong. See be/src/hooks/test-delivery.ts for why
        // it makes a real request rather than calling the handler.
        //
        // POST because it has an effect — the job runs, for real, with a
        // payload that says it is a test. A job that acts on what it receives
        // will act on this one, which is why the panel beside the button says
        // so rather than only the docs.
        if (url.pathname.startsWith("/api/jobs/") && url.pathname.endsWith("/test-delivery")
            && req.method === "POST") {
            const id = decodeURIComponent(
                url.pathname.slice("/api/jobs/".length, -"/test-delivery".length),
            );
            const job = jobById(id);
            if (!job) {
                send(res, 404, { message: `No job with id "${id}".` });
                return done(404);
            }
            if (job.webhook === undefined) {
                // 400 rather than 404: the job is real and the request is the
                // thing that makes no sense, and saying so beats the listener's
                // deliberate ambiguity — this endpoint is on the authenticated
                // side of the app, where a reader is entitled to a reason.
                send(res, 400, { message: `Job "${id}" declares no webhook.` });
                return done(400);
            }
            // Never fails as a request: every way this can go wrong is a fact
            // about the install that the caller asked to be told, so it comes
            // back 200 with the failure described rather than as an error the
            // page has to translate.
            send(res, 200, await sendTestDelivery(job));
            return done(200);
        }

        // Forget one job's memory, without touching any other job's.
        //
        // The alternative, and what this replaces, was telling people to delete
        // ~/.config/rn/job-state.json — the same act aimed at every job at
        // once. That was fine while rn had one polling job and became a quiet
        // trap when it had two: you delete the file to re-run one report and
        // silently re-trigger the other job's whole backlog.
        //
        // DELETE rather than POST because it removes a resource and nothing
        // else, and because it is idempotent in the way the method promises:
        // asking twice is not an error, and the second answer is zeroes.
        if (url.pathname.startsWith("/api/jobs/") && url.pathname.endsWith("/state")
            && req.method === "DELETE") {
            const id = decodeURIComponent(
                url.pathname.slice("/api/jobs/".length, -"/state".length),
            );
            const job = jobById(id);
            if (!job) {
                send(res, 404, { message: `No job with id "${id}".` });
                return done(404);
            }
            // Refused rather than raced. A run holds a staged handle that
            // commits when it finishes, so forgetting underneath it would be
            // undone moments later by writes the caller cannot see — and the
            // page would report a reset that did not survive the minute. See
            // state.open(): one handle at a time is the rule this rests on.
            if (running.isRunning(job.id)) {
                send(res, 409, {
                    id: job.id,
                    message:
                        `"${job.id}" is running. Its memory is committed when the run ends, ` +
                        `so forgetting now would be overwritten — wait for it to finish.`,
                });
                return done(409);
            }
            const removed = jobState.forget(job.id);
            send(res, 200, {
                id: job.id,
                cursors: removed.cursors,
                ids: removed.ids,
                // Said explicitly rather than left to the reader: "forgot 0
                // cursors" reads as a failure, and "nothing to forget" is what
                // actually happened on a job that has never run.
                wasEmpty: removed.cursors === 0 && removed.ids === 0,
            } satisfies StateResetResponse);
            return done(200);
        }

        // Run one job now. The only trigger that exists today — a scheduler is
        // the next front door onto the same runner, not a second runner. See
        // docs/jobs.md §3.
        if (url.pathname.startsWith("/api/jobs/") && req.method === "POST") {
            const id = decodeURIComponent(url.pathname.slice("/api/jobs/".length));
            const job = jobById(id);
            if (!job) {
                send(res, 404, { message: `No job with id "${id}".` });
                return done(404);
            }
            // The body is what this run is being asked to do — see JobInput in
            // shared/src/jobs.rs. An empty body is the ordinary case and reads
            // as {}.
            let body: unknown;
            try {
                body = await readJson(req);
            } catch (err) {
                send(res, 400, {
                    id: job.id,
                    message: err instanceof Error ? err.message : String(err),
                });
                return done(400);
            }

            // Checked here as well as in runJob, and this is the reason: a job
            // that starts and then fails on bad input has already made its
            // first side effect, and the caller deserves 400 with what was
            // wrong rather than 500 with what broke.
            const resolved = resolveInput(job, body);
            if (!resolved.ok) {
                send(res, 400, {
                    id: job.id,
                    message: resolved.errors.join("; "),
                    errors: resolved.errors,
                });
                return done(400);
            }

            try {
                const result = await runJob(job, "manual", undefined, body);
                send(res, 200, { id: job.id, ...result });
                return done(200);
            } catch (err) {
                // runJob has already logged this with a duration. The client
                // gets the message so a panel can show why rather than "500".
                send(res, 500, {
                    id: job.id,
                    message: err instanceof Error ? err.message : String(err),
                });
                return done(500);
            }
        }

        // What this install needs and whether it has it. Names, variables and
        // two booleans — never a value, not even a prefix or a length of one.
        // See docs/token-sec.md: a panel renders existence, and this endpoint
        // has no shape that could carry content even if a page asked.
        if (url.pathname === "/api/credentials" && req.method === "GET") {
            const state = credentialsFile.fileState();
            send(res, 200, {
                entries: credentialEntries(),
                // Display form. The absolute path stays here and is never
                // accepted back — the backend resolves its own file.
                path: displayPath(config.credentialsPath),
                exists: state.exists,
                ...(state.permissionWarning === undefined
                    ? {}
                    : { permissionWarning: state.permissionWarning }),
            } satisfies CredentialsResponse);
            return done(200);
        }

        // Set one. Write-only: the response says whether it took, and the
        // reader is told nothing it did not already send.
        //
        // The value is applied to this process before it is written to the
        // file, which is what arms redaction — see be/src/credentials-file.ts.
        // So it works immediately and there is no restart to tell anyone about.
        if (url.pathname.startsWith("/api/credentials/") && req.method === "PUT") {
            const name = decodeURIComponent(url.pathname.slice("/api/credentials/".length));
            let body: unknown;
            try {
                body = await readJson(req);
            } catch {
                // The parse error is not echoed: a malformed body containing a
                // credential would put it in the response and the log.
                send(res, 400, {
                    ok: false,
                    errors: ["the request body could not be read as JSON"],
                } satisfies CredentialSaveResponse);
                return done(400);
            }
            const value = (body as { value?: unknown })?.value;
            if (typeof value !== "string") {
                send(res, 400, {
                    ok: false,
                    errors: ["the body must be {\"value\": \"…\"}"],
                } satisfies CredentialSaveResponse);
                return done(400);
            }
            const result = credentialsFile.set(name, value);
            if (result.errors.length > 0) {
                send(res, 422, {
                    ok: false,
                    errors: result.errors,
                } satisfies CredentialSaveResponse);
                return done(422);
            }
            send(res, 200, {
                ok: true,
                entry: credentialsFile.describe(name, declaredBy(name)),
            } satisfies CredentialSaveResponse);
            return done(200);
        }

        // Remove one, from this process and from the file. Idempotent in the
        // way DELETE promises: asking twice is a 404 and it is gone either way.
        if (url.pathname.startsWith("/api/credentials/") && req.method === "DELETE") {
            const name = decodeURIComponent(url.pathname.slice("/api/credentials/".length));
            if (!credentialsFile.clear(name)) {
                send(res, 404, {
                    ok: false,
                    errors: [`Nothing is set for "${name}".`],
                } satisfies CredentialSaveResponse);
                return done(404);
            }
            send(res, 200, {
                ok: true,
                entry: credentialsFile.describe(name, declaredBy(name)),
            } satisfies CredentialSaveResponse);
            return done(200);
        }

        // The webhooks made on the page, as opposed to the ones a job declares
        // for itself. Both kinds answer on the hooks listener; only these can be
        // changed from here, which is why only these are on this route.
        //
        // Deliberately carries no URL. The route (`POST /api/hooks/demo`) is
        // sent; the host it hangs off is a tunnel address, which is a bearer
        // capability — see shared/src/webhooks.rs. The page shows the port and
        // the path and leaves the person who knows their own tunnel to put the
        // two together.
        if (url.pathname === "/api/webhooks" && req.method === "GET") {
            const hooks = hooksHealth();
            send(res, 200, {
                webhooks: webhooks.describeAll(),
                // From the catalogue this process actually has, so a routing
                // control cannot offer a job that was renamed out from under it.
                jobs: JOBS.map((j) => j.id),
                defaults: webhooks.DEFAULTS,
                max: webhooks.MAX_WEBHOOKS,
                // A webhook saved against a listener that failed to bind is
                // configuration with nothing behind it, and the provider's own
                // retry log is otherwise the only place that shows.
                listening: hooks.listening,
                port: hooks.port,
            } satisfies WebhooksResponse);
            return done(200);
        }

        // Make or replace one. PUT rather than POST because the id is in the
        // path and the body is the whole definition: sending it twice leaves
        // the same webhook, which is what the method promises.
        //
        // Whole, not patched — a merge would let a field the form did not send
        // survive invisibly, and for `lookup` or `routes` that means an endpoint
        // doing something that is no longer written down anywhere.
        if (url.pathname.startsWith("/api/webhooks/") && req.method === "PUT") {
            const id = decodeURIComponent(url.pathname.slice("/api/webhooks/".length));
            let body: unknown;
            try {
                body = await readJson(req);
            } catch (err) {
                send(res, 400, {
                    ok: false,
                    errors: [err instanceof Error ? err.message : String(err)],
                } satisfies WebhookSaveResponse);
                return done(400);
            }
            // `replacing` is the id in the path and the body's `id` is what it
            // becomes, so renaming is an ordinary save rather than a delete and
            // a create — which would drop the webhook for as long as the two
            // requests took, on a URL a provider may be calling.
            const exists = webhooks.byId(id) !== undefined;
            const result = webhooks.put(body as WebhookDef, exists ? id : undefined);
            if (result.errors.length > 0) {
                // 422, not 400: the request was understood and the definition
                // was refused, and the page has a list of sentences to put under
                // the fields rather than one message to put at the top.
                send(res, 422, { ok: false, errors: result.errors } satisfies WebhookSaveResponse);
                return done(422);
            }
            send(res, exists ? 200 : 201, {
                ok: true,
                webhook: webhooks.describe(result.def!),
            } satisfies WebhookSaveResponse);
            return done(exists ? 200 : 201);
        }

        // Remove one. Idempotent in the way DELETE promises — asking twice is a
        // 404 rather than an error, and the endpoint is gone either way.
        if (url.pathname.startsWith("/api/webhooks/") && req.method === "DELETE") {
            const id = decodeURIComponent(url.pathname.slice("/api/webhooks/".length));
            if (!webhooks.remove(id)) {
                send(res, 404, {
                    ok: false,
                    errors: [`No webhook with id "${id}".`],
                } satisfies WebhookSaveResponse);
                return done(404);
            }
            send(res, 200, { ok: true } satisfies WebhookSaveResponse);
            return done(200);
        }

        if (url.pathname === "/api/restart" && req.method === "POST") {
            if (!isSupervised()) {
                // Exiting unsupervised would just kill the app. Say so rather
                // than leaving the user with a dead server and no explanation.
                send(res, 409, {
                    ok: false,
                    // Stated rather than left out: a refusal is not a restart
                    // that happens to be pending, and the type says so.
                    scheduled: false,
                    error: "not supervised",
                    message:
                        "No launcher is managing this process, so it cannot restart itself. Start it with the rn binary, or restart manually.",
                } satisfies RestartOutcome);
                return done(409);
            }
            // "now" restarts regardless; the default waits for running work.
            // Aborting a long automation to apply a setting is the failure
            // mode this guards against.
            const when = url.searchParams.get("when") === "now" ? "now" : "idle";
            const runningJobs = running.list();

            if (when === "idle" && runningJobs.length > 0) {
                if (!restartWhenIdle) {
                    restartWhenIdle = true;
                    running.whenIdle(() => {
                        step("restart-when-idle-fired", {});
                        doRestart();
                    });
                }
                send(res, 202, {
                    ok: true,
                    scheduled: true,
                    message: "Restart scheduled — waiting for running work to finish.",
                    running: runningJobs,
                } satisfies RestartOutcome);
                return done(202);
            }

            send(res, 200, {
                ok: true,
                scheduled: false,
                message: "restarting",
                aborted: when === "now" ? runningJobs : [],
            } satisfies RestartOutcome);
            done(200);
            step("restart-requested", { when, aborting: runningJobs.length });
            doRestart();
            return;
        }

        if (url.pathname === "/api/settings" && req.method === "PUT") {
            void (async () => {
                try {
                    const body = (await readJson(req)) as Settings;
                    const errors = validateAll(body);
                    if (errors.length > 0) {
                        send(res, 400, { ok: false, errors } satisfies SaveResponse);
                        return done(400);
                    }
                    save(config.settingsPath, body);
                    // Apply what can take effect now, so the UI's "(immediate)"
                    // label is true rather than aspirational.
                    const applied = applyRuntimeSettings(body);
                    const restart = needsRestart(Object.keys(body)).map((p) => p.id);
                    step("settings-saved", { applied, restart });
                    send(res, 200, {
                        ok: true,
                        applied,
                        restartRequired: restart,
                    } satisfies SaveResponse);
                    done(200);
                } catch (err) {
                    send(res, 400, {
                        ok: false,
                        errors: [{ id: "-", message: String(err) }],
                    } satisfies SaveResponse);
                    done(400);
                }
            })();
            return;
        }

        send(res, 404, { error: "not found", path: url.pathname });
        done(404);
    });
}

// Settings saved earlier must take effect on this process too, not just on
// the process that saved them.
const bootApplied = applyRuntimeSettings(load(config.settingsPath));

/** Exit cleanly when the launcher (or a service manager) asks us to stop. */
/**
 * Stop listening — on both ports.
 *
 * There are three places the API shuts down (a signal, /api/stop, and a
 * restart) and a second listening socket is a second open handle: miss it in
 * any one of them and the process stays alive with nothing serving it, which
 * from the launcher looks like a backend that refuses to stop. One function so
 * there is one thing to get right.
 */
function closeListeners(srv: ReturnType<typeof createApp>, done?: () => void): void {
    hooks.close();
    srv.close(done);
}

function installSignalHandlers(srv: ReturnType<typeof createApp>): void {
    let stopping = false;
    for (const signal of ["SIGTERM", "SIGINT"] as const) {
        process.on(signal, () => {
            if (stopping) return;
            stopping = true;
            step("stopping", { signal, running: running.count() });
            // Stop firing new work the moment we know we are going down.
            scheduler.stop();
            // Stop accepting connections, then exit 0 so the launcher knows
            // this was intentional and does not restart us.
            closeListeners(srv, () => process.exit(0));
            // Don't hang forever on a keep-alive connection.
            setTimeout(() => process.exit(0), 3000).unref();
        });
    }
}

// Fail closed before a socket exists, not after. A refusal that arrives once
// the port is already open has already published what it was refusing.
const refusal = remoteBindRefusal(config.host, config.allowRemote);
if (refusal !== null) {
    error("bind-refused", { host: config.host, port: config.port });
    console.error(refusal);
    process.exit(1);
}

const server = createApp();
installSignalHandlers(server);

// One route, its own port, and the thing a tunnel points at. See
// be/src/hooks/server.ts for why it is not a route on the server above.
const hooks = createHookApp();
// The second front door onto runJob. Started after listen so a slow boot
// cannot fire a job before the API can report that it is running.
/**
 * A bind failure on the API is terminal, and the asymmetry with the hooks
 * listener is deliberate.
 *
 * The hooks listener degrades because everything else still works without it.
 * This one cannot: the API is the only way anything — the frontend, the
 * launcher's status, a person with curl — learns what the process is doing. A
 * backend that is running and unreachable is worse than one that stopped and
 * said why, because only the second gets investigated.
 *
 * So it explains itself and exits EXIT_FATAL, which asks the launcher not to
 * retry. Before this the event was unhandled: Node threw, the supervisor read
 * an ordinary crash, and the operator got five restarts and a message about
 * their frequency rather than the sentence below.
 */
server.on("error", (err: NodeJS.ErrnoException) => {
    const cause = err.code === "EADDRINUSE"
        ? `${config.host}:${config.port} is already in use — another rn, or something else holding the port`
        : err.code === "EACCES"
        ? `not allowed to bind ${config.host}:${config.port} — ports below 1024 need privileges`
        : (err.message ?? String(err));
    error("listen-failed", { host: config.host, port: config.port, code: err.code ?? null, cause });
    // stderr as well as the log: the launcher prints this straight through, and
    // it is the line someone starting rn by hand actually sees.
    process.stderr.write(`rn: cannot start the API — ${cause}\n`);
    process.exit(EXIT_FATAL);
});

server.listen(config.port, config.host, () => {
    // Same host as the API, so the guard above covers both. Started here rather
    // than independently because a hooks port that outlives the API would take
    // deliveries for a process that can no longer report what it did with them.
    // startHooks rather than hooks.listen: a failed bind here used to throw an
    // unhandled error event and take the API down with it. See its doc.
    startHooks(hooks, config.hooksPort, config.host, () => {
        step("hooks-listening", {
            url: `http://${config.host}:${config.hooksPort}`,
            route: "POST /api/hooks/:id",
            jobs: JOBS.filter((j) => j.webhook !== undefined).length,
        });
    });
    scheduler.start();
    step("listening", {
        url: `http://${config.host}:${config.port}`,
        node: process.version,
        execPath: displayPath(process.execPath),
        settingsPath: displayPath(config.settingsPath),
        applied: bootApplied,
    });
});
