/**
 * All defaults and environment reading live here, in one place, so that
 * "where do I change this?" always has the same answer.
 *
 * Precedence: real environment variables win over .env, which wins over the
 * defaults below.
 */
export const config = {
    /**
     * error | warn | info | debug. Read once here; `log.ts` owns the live value
     * because the registry parameter can move it without a restart.
     */
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
     * The hooks listener's port — the one a tunnel points at.
     *
     * Separate from `port` on purpose, and it is the whole security design of
     * the webhook feature. The API on `port` has no authentication, so exposing
     * *it* through a tunnel would publish `PUT /api/settings` and
     * `POST /api/jobs/:id` to the internet. The hooks server serves one route
     * and has no path to any of that, so the boundary is structural rather than
     * a rule in a tunnel's config file.
     *
     * Shares `host`: both are loopback, and `remoteBindRefusal` covers both.
     */
    hooksPort: Number(process.env.BACKEND_HOOKS_PORT ?? 3011),

    /**
     * The click tracker's port — the second thing a tunnel points at, and the
     * only socket in rn that answers an unauthenticated stranger on purpose.
     *
     * Its own port rather than a route on `hooksPort`, and the reason is the
     * one `hooksPort` states for itself. That listener's security argument is
     * that it serves exactly one route and has no path to anything else, which
     * `docs/tunnel.md` proves by measuring `GET /api/settings` answering 404
     * through the public URL. A public GET namespace on the same port spends
     * that argument: the guarantee stops being "the route does not exist" and
     * becomes "the routing is correct", which is a weaker claim about a bigger
     * surface — and a path-parsing bug would then land on the port a provider
     * delivers signed webhooks to.
     *
     * Separate also means separable: the tracker can be stopped, rate-limited
     * or unpublished without touching webhook delivery, and it is the only part
     * of rn whose traffic scales with how many people were mailed.
     *
     * See docs/link-tracking.md §3.
     */
    trackerPort: Number(process.env.BACKEND_TRACKER_PORT ?? 3012),

    /**
     * Permission to bind a routable address — see `remoteBindRefusal` below.
     *
     * Read here rather than at the point of use, which is where it was and
     * should not have been. This module's first line promises that all
     * environment reading lives in it, and a variable read straight out of
     * `process.env` in server.ts was invisible to the check that keeps
     * `.env.example` honest — so the one setting whose refusal message tells
     * you to go and set it was the one the reference did not list.
     *
     * Deliberately not defaulted. `remoteBindRefusal` treats exactly `"1"` as
     * permission and everything else — including `undefined` — as refusal, and
     * a default here would be a second place for that rule to live.
     */
    allowRemote: process.env.RN_ALLOW_REMOTE,

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
     * Where a job's cursors are kept between runs — see be/src/jobs/state.ts.
     *
     * Its own file rather than a section of job-runs.json, because the two have
     * opposite lifetimes. The run history is a bounded log that is expected to
     * lose its oldest entries and costs nothing when it does; a cursor is a
     * single value that must survive indefinitely, and losing one means the next
     * run reprocesses everything its source still holds. Sharing a file would
     * mean one truncation or one corrupt write taking both.
     */
    jobStatePath:
        process.env.RN_JOB_STATE_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/job-state.json`,

    /**
     * Where webhooks made on Config → Jobs are kept — see be/src/webhooks.ts.
     *
     * Its own file, beside the others and for the same reason: the install tree
     * is replaced wholesale on upgrade, and an endpoint a provider is already
     * calling must not disappear because rn was updated. Separate from
     * settings.json because these are not settings — nothing here has a default
     * that would be correct if the file went missing, and a lost definition is a
     * URL somebody else's system still POSTs to and now gets a 404 from.
     */
    webhooksPath:
        process.env.RN_WEBHOOKS_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/webhooks.json`,

    /**
     * The credentials file — the one the launcher reads and hands to this
     * process as `RN_SECRET_*` variables.
     *
     * Named here because Config → Jobs can now write it. The backend does not
     * read values out of it at startup: it gets those from its environment, as
     * it always has. What it needs the path for is the other direction, and the
     * one warning nobody sees — the launcher's "readable by others" line goes
     * to stderr at boot, which is not where anyone is looking.
     */
    credentialsPath:
        process.env.RN_CREDENTIALS_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/credentials`,

    /**
     * Minted tracking links and the clicks that came back — see
     * be/src/tracker/store.ts.
     *
     * JSONL rather than JSON, and its own file rather than a section of any
     * other, because it is the only store here written by a request handler
     * from outside the machine. The others are rewritten wholly when a person
     * presses save; a crash during one of those costs a file, and a crash
     * during an append here costs a line.
     */
    trackerStorePath:
        process.env.RN_TRACKER_STORE_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/link-tracking.jsonl`,

    /**
     * The origin tracked links are minted against — what actually appears in
     * the mail.
     *
     * Defaults to this machine's own tracker port, which is deliberately a
     * **useless** default outside the machine: a link to 127.0.0.1 fails
     * visibly in the recipient's browser rather than silently pointing
     * somewhere wrong. Set it to the public origin the tunnel serves, and
     * nothing else — `docs/link-tracking.md` §5 is about what a hostname in a
     * link costs, and a port number in it reads as phishing to filters and to
     * people.
     *
     * No trailing slash; `/<id>` is appended.
     */
    trackerBaseUrl:
        process.env.RN_TRACKER_BASE_URL ??
        `http://127.0.0.1:${Number(process.env.BACKEND_TRACKER_PORT ?? 3012)}/t`,

    /**
     * How long the tracker remembers *who*, in days.
     *
     * Not how long a link lives. Links never expire: one sits in somebody's
     * mailbox and may be clicked years later, and expiring the id turns that
     * into a 404 in mail a person kept — losing analytics is an annoyance,
     * breaking a link somebody was sent is a fault. What expires is the
     * recipient recorded against a link, and the clicks themselves.
     *
     * Ninety days because it is long enough to answer "did that campaign
     * work" and short enough that a store nobody has looked at in a year is
     * not still naming people. See docs/link-tracking.md §6, including what
     * this deliberately does not fix: the distinct links are already in
     * mailboxes, so retention bounds what rn knows and not what the mail
     * reveals.
     */
    trackerRetentionDays: Number(process.env.RN_TRACKER_RETENTION_DAYS ?? 90),

    /**
     * Whether the operator has accepted that the tracker's hostname is lent
     * rather than owned — `docs/link-tracking.md` §3, option 3.
     *
     * It waives exactly one check and no others. A `.ts.net` name still cannot
     * be minted against over http, from a bare address, or with a port in it:
     * those are different mistakes and this says nothing about them. The
     * variable is long and unabbreviated on purpose — it is not a thing to set
     * while trying to get a send working, and every link minted while it is on
     * outlives the setting.
     */
    trackerAcceptBorrowedHostname: process.env.RN_TRACKER_ACCEPT_BORROWED_HOSTNAME === "1",

    /**
     * Which (send, recipient) pairs the send job has already attempted.
     *
     * Its own file rather than a section of the link store, because the two
     * have different lifetimes: links are pruned on a retention clock and these
     * markers must outlive that, or a pruned send becomes sendable again.
     */
    trackerSentPath:
        process.env.RN_TRACKER_SENT_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/link-sends.jsonl`,

    /** SMTP host the send job connects to. */
    smtpHost: process.env.RN_SMTP_HOST ?? "smtp.gmail.com",

    /**
     * SMTP port. 465 is implicit TLS, which is what this connects with.
     *
     * 587 (STARTTLS) works too and is what some providers require, but it opens
     * in plaintext and upgrades, so a network that strips the upgrade leaves
     * the session readable. 465 cannot be downgraded that way.
     */
    smtpPort: Number(process.env.RN_SMTP_PORT ?? 465),

    /**
     * The address mail is sent from, and the SMTP username.
     *
     * Not a credential: it is printed on every message that arrives, so it is
     * not a secret, and holding it as one would only hide it from the pages
     * that should say which account is sending.
     */
    smtpUser: process.env.RN_SMTP_USER ?? "",

    /**
     * Per-job settings changed from Config → Jobs, keyed by job id.
     *
     * Its own file rather than a section of settings.json, for the reason
     * webhooks.json is its own: nothing in it is a registry parameter, nothing
     * in it has a default that would be right if the file vanished, and the
     * keys are job ids the registry knows nothing about. Deleting it puts every
     * job back to exactly what its code declares, which is what makes editing
     * one safe to offer at all.
     */
    jobOverridesPath:
        process.env.RN_JOB_OVERRIDES_PATH ??
        `${process.env.HOME ?? "."}/.config/rn/job-overrides.json`,

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
