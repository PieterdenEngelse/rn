/**
 * All defaults and environment reading live here, in one place, so that
 * "where do I change this?" always has the same answer.
 *
 * Precedence: real environment variables win over .env, which wins over the
 * defaults below.
 */
export const config = {
    /** error | warn | info | debug */
    logLevel: process.env.LOG_LEVEL ?? "info",

    /**
     * Safety switch, ON by default. A misconfigured automation should do
     * nothing rather than something irreversible. Note the inverted check:
     * only the exact string "false" turns it off.
     */
    dryRun: process.env.DRY_RUN !== "false",

    /**
     * API server bind address. Loopback by default, and see
     * `remoteBindRefusal` below for why widening it is a two-part act.
     */
    host: process.env.BACKEND_HOST ?? "127.0.0.1",
    port: Number(process.env.BACKEND_PORT ?? 3010),

    /**
     * Where user settings are stored. Never inside the install directory —
     * that gets replaced wholesale on upgrade.
     */
    settingsPath:
        process.env.RN_SETTINGS_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/settings.json`,

    /**
     * Where the rolling metric history is kept between runs. Beside the
     * settings and for the same reason: the install tree is replaced on
     * upgrade, and losing the history to an upgrade is the same bug as losing
     * it to a restart.
     */
    historyPath:
        process.env.RN_HISTORY_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/history.json`,

    /**
     * Where each job run is recorded. Beside the settings and the metric
     * history, and for the same reason: the install tree is replaced wholesale
     * on upgrade, and a record of what ran that an upgrade erases is not a
     * record.
     */
    jobRunsPath:
        process.env.RN_JOB_RUNS_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/job-runs.json`,

    /**
     * Where V8 drops its profiling artifacts, and how long they are kept.
     *
     * `--cpu-prof`, `--heap-prof` and `--prof` write into the *current working
     * directory* — not a temp dir, not anywhere configurable — so the default
     * is cwd because that is where they provably land. A few runs of the
     * profiler leave a couple of megabytes behind, and nothing removes them.
     *
     * Seven days keeps the artifacts from a session you are still thinking
     * about and clears the ones you have forgotten. See jobs/prune-profiles.ts.
     */
    profileDir: process.env.RN_PROFILE_DIR ?? process.cwd(),
    profileMaxAgeDays: Number(process.env.RN_PROFILE_MAX_AGE_DAYS ?? 7),

    /**
     * Dev only: the origins the dx dev server may be reached at. Unused in a
     * packaged install, where the launcher serves both halves from one origin.
     *
     * A list rather than one string because http://localhost:1790 and
     * http://127.0.0.1:1790 are the same server and two different origins to a
     * browser. Both are reachable, both are things a person types, and allowing
     * only one meant the app worked or refused to talk to itself depending on
     * which spelling was in the address bar — with a failure that surfaces as
     * "backend unreachable" while the backend is plainly running.
     *
     * Comma-separated in RN_CORS_ORIGIN, which replaces the list rather than
     * extending it, so a packaged or proxied deployment can pin exactly one.
     */
    corsOrigins: (
        process.env.RN_CORS_ORIGIN ?? "http://localhost:1790,http://127.0.0.1:1790"
    )
        .split(",")
        .map((o) => o.trim())
        .filter(Boolean),
} as const;

/**
 * Addresses that cannot be routed to from another machine.
 *
 * The whole of 127.0.0.0/8 is loopback, not just 127.0.0.1 — a host bound to
 * 127.0.0.2 is every bit as unreachable, and calling it remote would be a
 * refusal nobody could act on.
 */
function isLoopback(host: string): boolean {
    return host === "localhost" || host === "::1" || host.startsWith("127.");
}

/**
 * Why this process must not listen, or `null` when it may.
 *
 * There is no authentication on this API. `POST /api/jobs/:id` runs an
 * automation against the user's filesystem, `/api/stop` and `/api/restart`
 * control the process, and `/api/jobs/:id/source` reads files back. That is
 * defensible for exactly as long as the socket cannot be reached from another
 * machine — so binding wider is refused unless the operator also says, in a
 * second place, that they meant it.
 *
 * Fail closed, and deliberately two-part. Before this, the entire security
 * position rested on one default that a one-character edit could flip with
 * nothing reporting it: the Connection page could say "this machine only", and
 * it was describing a value rather than an invariant. Now it is describing one.
 *
 * Pure, and exported for the tests: the rule is the point, and it should be
 * checkable without opening a socket.
 */
export function remoteBindRefusal(
    host: string,
    allowRemote: string | undefined,
): string | null {
    if (isLoopback(host)) return null;
    if (allowRemote === "1") return null;
    return (
        `rn: refusing to listen on ${host} — this API has no authentication, ` +
        "and anything that can reach it can run your automations.\n" +
        "  If you want it reachable from another machine, the safe answer is " +
        "almost always a tunnel (ssh -L, WireGuard, Tailscale) to the loopback " +
        "socket, which needs no change here. See docs/network.md.\n" +
        "  To bind wider anyway, set RN_ALLOW_REMOTE=1 in be/.env beside " +
        "BACKEND_HOST, and put authentication in front of it."
    );
}
