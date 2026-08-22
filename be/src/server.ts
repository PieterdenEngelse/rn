/**
 * The API the frontend reads. Deliberately small: node:http, no framework.
 *
 * GET  /api/health    liveness
 * GET  /api/params    the parameter registry + live values + saved settings
 * PUT  /api/settings  save settings (validated); reports what needs a restart
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
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

/**
 * Exit code that asks the launcher for a restart. Must match EXIT_RESTART in
 * launcher/src/lib.rs — the two halves of one protocol.
 */
const EXIT_RESTART = 75;
import { step } from "./log.ts";
import * as jobs from "./jobs.ts";
import { collect as collectNodeMetrics, lifetimeDelay } from "./node_metrics.ts";
import { withDistribution } from "./node_history.ts";

function send(res: ServerResponse, code: number, body: unknown): void {
    const json = JSON.stringify(body);
    res.writeHead(code, {
        "content-type": "application/json; charset=utf-8",
        "content-length": Buffer.byteLength(json),
        // The dev frontend runs on a different port (dx serve :1790), so the
        // browser treats it as cross-origin. Dev-only convenience: in a packaged
        // install the launcher serves both from one origin and this is unused.
        "access-control-allow-origin": config.corsOrigin,
        "access-control-allow-methods": "GET, PUT, OPTIONS",
        "access-control-allow-headers": "content-type",
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
    return createServer((req, res) => {
        const url = new URL(req.url ?? "/", `http://${req.headers.host ?? "localhost"}`);
        const started = Date.now();

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
                execPath: process.execPath,
                settingsPath: config.settingsPath,
                url: `http://${config.host}:${config.port}`,
                jobs: jobs.count(),
                restartPending: restartWhenIdle,
                // Lets the header light go amber without a second request.
                pendingCount: pendingRestart(load(config.settingsPath)).length,
            });
            return done(200);
        }

        if (url.pathname === "/api/stop" && req.method === "POST") {
            const runningJobs = jobs.list();
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
            send(res, 200, { running: jobs.list(), restartPending: restartWhenIdle });
            return done(200);
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
            const runningJobs = jobs.list();

            if (when === "idle" && runningJobs.length > 0) {
                if (!restartWhenIdle) {
                    restartWhenIdle = true;
                    jobs.whenIdle(() => {
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
            step("stopping", { signal, running: jobs.count() });
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
server.listen(config.port, config.host, () => {
    step("listening", {
        url: `http://${config.host}:${config.port}`,
        node: process.version,
        execPath: process.execPath,
        settingsPath: config.settingsPath,
        applied: bootApplied,
    });
});
