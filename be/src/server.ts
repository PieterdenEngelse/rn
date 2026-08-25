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
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { readFile } from "node:fs/promises";
import { RUNTIME_PARAMS, WITHHELD } from "./runtime-params.ts";
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
import { config } from "./config.ts";
import { display as displayPath } from "./paths.ts";

/**
 * Exit code that asks the launcher for a restart. Must match EXIT_RESTART in
 * launcher/src/lib.rs — the two halves of one protocol.
 */
const EXIT_RESTART = 75;
import { step } from "./log.ts";
import * as running from "./running.ts";
import {
    JOBS,
    jobById,
    runJob,
    scheduler,
    history as jobHistory,
    DEFAULT_TIMEOUT_MS,
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

/** Set once a restart is queued behind running work. */
let restartWhenIdle = false;

/**
 * Exit with the code the launcher watches for. It rebuilds the environment
 * from the settings as they are now and starts a fresh process.
 */
function doRestart(): void {
    server.close();
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
        res.setHeader("access-control-allow-methods", "GET, POST, PUT, OPTIONS");
        res.setHeader("access-control-allow-headers", "content-type");

        const done = (code: number): void => {
            step("http", { method: req.method, path: url.pathname, code, ms: Date.now() - started });
        };

        if (req.method === "OPTIONS") {
            send(res, 204, {});
            return done(204);
        }

        if (url.pathname === "/api/health") {
            send(res, 200, { status: "ok", node: process.version });
            return done(200);
        }

        if (url.pathname === "/api/params" && req.method === "GET") {
            const settings = load(config.settingsPath);
            send(res, 200, {
                params: RUNTIME_PARAMS,
                withheld: WITHHELD,
                effective: effectiveValues(),
                settings,
                supervised: isSupervised(),
                pending: pendingRestart(settings),
            });
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
            });
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
                });
                return done(409);
            }
            send(res, 200, { ok: true, message: "stopping", aborted: runningJobs });
            done(200);
            step("stop-requested", { force, aborting: runningJobs.length });
            server.close();
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
                    timeoutMs: j.timeoutMs ?? DEFAULT_TIMEOUT_MS,
                    // Same argument as the schedule above: a failure path
                    // nobody can see is indistinguishable from no failure
                    // path, and the user only finds out which they had when
                    // the job fails.
                    ...(j.onFailure === undefined ? {} : { onFailure: j.onFailure }),
                })),
                dryRun: config.dryRun,
                // The knobs every run is subject to, whichever job it is.
                // Resolved here for the same reason `timeoutMs` above is: they
                // are constants in be/src/jobs/, and Config → Jobs reports what
                // this process actually has rather than a copy that drifts.
                config: {
                    defaultTimeoutMs: DEFAULT_TIMEOUT_MS,
                    schedulerTickMs: scheduler.TICK_MS,
                    historyCapacity: jobHistory.CAPACITY,
                    failureCapacity: jobHistory.FAILURE_CAPACITY,
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
                recent: jobHistory
                    .list(25)
                    .map((run) => ({ ...run, outcome: jobHistory.outcome(run) })),
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
            try {
                const result = await runJob(job);
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

        if (url.pathname === "/api/restart" && req.method === "POST") {
            if (!isSupervised()) {
                // Exiting unsupervised would just kill the app. Say so rather
                // than leaving the user with a dead server and no explanation.
                send(res, 409, {
                    ok: false,
                    error: "not supervised",
                    message:
                        "No launcher is managing this process, so it cannot restart itself. Start it with the rn binary, or restart manually.",
                });
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
                });
                return done(202);
            }

            send(res, 200, {
                ok: true,
                scheduled: false,
                message: "restarting",
                aborted: when === "now" ? runningJobs : [],
            });
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
                        send(res, 400, { ok: false, errors });
                        return done(400);
                    }
                    save(config.settingsPath, body);
                    // Apply what can take effect now, so the UI's "(immediate)"
                    // label is true rather than aspirational.
                    const applied = applyRuntimeSettings(body);
                    const restart = needsRestart(Object.keys(body)).map((p) => p.id);
                    step("settings-saved", { applied, restart });
                    send(res, 200, { ok: true, applied, restartRequired: restart });
                    done(200);
                } catch (err) {
                    send(res, 400, { ok: false, errors: [{ id: "-", message: String(err) }] });
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
            srv.close(() => process.exit(0));
            // Don't hang forever on a keep-alive connection.
            setTimeout(() => process.exit(0), 3000).unref();
        });
    }
}

const server = createApp();
installSignalHandlers(server);
// The second front door onto runJob. Started after listen so a slow boot
// cannot fire a job before the API can report that it is running.
server.listen(config.port, config.host, () => {
    scheduler.start();
    step("listening", {
        url: `http://${config.host}:${config.port}`,
        node: process.version,
        execPath: displayPath(process.execPath),
        settingsPath: displayPath(config.settingsPath),
        applied: bootApplied,
    });
});
