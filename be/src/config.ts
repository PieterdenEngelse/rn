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

    /** Dev only: the dx serve origin. Unused in a packaged install. */
    corsOrigin: process.env.RN_CORS_ORIGIN ?? "http://localhost:1790",
} as const;
