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

    /** API server bind address. */
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
