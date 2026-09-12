/**
 * Tokens: when a credential stops working, and what stops with it.
 *
 * The credentials board answers "is it set". This answers what
 * `config_connection.rs` admits in the OAuth panel: a credential that is set
 * is not a credential that works, and when an expired one takes an
 * integration down, the evidence is an ordinary 401 in one job's log with
 * nothing naming the cause.
 *
 * Everything here is derived from what the process already has — the token's
 * own `exp` claim, and runs that already happened. Nothing calls a provider,
 * so opening the page costs no request against anyone's rate limit, and a
 * page that is merely open cannot turn into traffic.
 *
 * **No value leaves this module.** `jwtExpiry` is handed a secret and returns
 * one number; nothing else here ever sees one, and failure messages go out
 * through `secrets.redact` as everywhere else.
 */
import type { CredentialEntry, JobRun, TokenEntry, TokenRun, TokensResponse } from "./generated/wire.ts";
import * as secrets from "./secrets.ts";

/** How far back run history is read. */
export const WINDOW_DAYS = 30;

/**
 * The `exp` claim of a JWT, or why there is not one.
 *
 * Deliberately narrow: three dot-separated parts, a decodable middle, an `exp`
 * that is a number. Anything else is "not a JWT", which is the honest answer
 * for a GitHub token or an app password — they carry no expiry a machine can
 * read, and saying so is better than implying they last forever.
 */
export function jwtExpiry(value: string): { atMs: number } | { unknown: string } {
    const parts = value.split(".");
    if (parts.length !== 3) return { unknown: "not a JWT: no expiry to read" };
    let payload: unknown;
    try {
        payload = JSON.parse(Buffer.from(parts[1]!, "base64url").toString("utf8"));
    } catch {
        return { unknown: "not a JWT: the payload does not decode" };
    }
    if (typeof payload !== "object" || payload === null) {
        return { unknown: "not a JWT: the payload is not an object" };
    }
    const exp = (payload as { exp?: unknown }).exp;
    if (typeof exp !== "number" || !Number.isFinite(exp)) {
        return { unknown: "a JWT, but with no exp claim" };
    }
    return { atMs: exp * 1000 };
}

/**
 * Whether a failure reads as a refused credential.
 *
 * A guess, and it says so on the page: job errors are prose from whatever
 * library made the call. Worth making anyway — the alternative is the reader
 * grepping run history by hand for the one word that means "the token".
 */
const AUTH_FAILURE =
    /\b(401|403)\b|unauthoriz|unauthenticat|forbidden|invalid[\s_-]*(token|grant|credential|api[\s_-]*key)|bad credentials|token (?:has )?(?:expired|revoked)|expired[\s_-]*token/i;

export function isAuthFailure(error: string | undefined | null): boolean {
    return typeof error === "string" && AUTH_FAILURE.test(error);
}

function runOf(run: JobRun): TokenRun {
    return {
        jobId: run.jobId,
        atMs: run.startedAt,
        ...(run.error === undefined || run.error === null
            ? {}
            : { error: secrets.redact(String(run.error)) }),
    };
}

/**
 * Expiries out of an rclone config, one row per remote that holds a token.
 *
 * rn does not use rclone, and depends on it anyway: the Drive and OneDrive
 * mounts are two of the user units the bootstrap enables, and when their
 * tokens lapse the mounts go quiet rather than loud. These are also the only
 * tokens on this machine that say when they die — everything rn declares is a
 * personal access token or an app password, which carry no expiry at all.
 *
 * Read for `expiry` and nothing else. The access and refresh tokens sit on the
 * same line and never leave this function: it returns a remote's name, its
 * type, and an instant.
 */
export function rcloneExpiries(conf: string, nowMs: number): TokenEntry[] {
    const out: TokenEntry[] = [];
    let name: string | undefined;
    let type: string | undefined;
    let expiry: string | undefined;

    const flush = () => {
        if (name === undefined || expiry === undefined) return;
        const atMs = Date.parse(expiry);
        if (Number.isNaN(atMs)) return;
        out.push({
            name: type === undefined ? name : `${name} (${type})`,
            origin: "rclone",
            set: true,
            inFile: true,
            declaredBy: [],
            authFailures: 0,
            expiry: { atMs, inSeconds: (atMs - nowMs) / 1000, source: "rclone" },
        });
    };

    for (const raw of conf.split("\n")) {
        const line = raw.trim();
        const section = /^\[(.+)\]$/.exec(line);
        if (section) {
            flush();
            name = section[1];
            type = undefined;
            expiry = undefined;
            continue;
        }
        const t = /^type\s*=\s*(\S+)/.exec(line);
        if (t) type = t[1];
        // The token is one JSON object on one line. Only its expiry is read,
        // by a pattern narrow enough that it cannot match the tokens beside it.
        const e = /"expiry"\s*:\s*"([^"]+)"/.exec(line);
        if (e) expiry = e[1];
    }
    flush();
    return out;
}

/**
 * Build the board.
 *
 * `valueOf` is injected rather than imported so a test can exercise expiry
 * without putting a token in the environment — and so the one place a secret
 * is read stays visible in the caller.
 */
export function build(opts: {
    entries: CredentialEntry[];
    runs: JobRun[];
    nowMs: number;
    windowDays?: number;
    valueOf: (name: string) => string | undefined;
    /// The contents of rclone.conf, when there is one. Passed in rather than
    /// read here so a test needs no file and the one place rn reads another
    /// tool's config stays visible in the caller.
    rcloneConf?: string | undefined;
}): TokensResponse {
    const windowDays = opts.windowDays ?? WINDOW_DAYS;
    const since = opts.nowMs - windowDays * 24 * 60 * 60 * 1000;
    const inWindow = opts.runs.filter((r) => r.startedAt >= since);

    const entries: TokenEntry[] = opts.entries.map((c) => {
        const mine = inWindow.filter((r) => c.declaredBy.includes(r.jobId));
        // Newest first, so "last" is the head of each list.
        const byNewest = [...mine].sort((a, b) => b.startedAt - a.startedAt);
        const success = byNewest.find(
            (r) => (r.error === undefined || r.error === null) && !r.skipped && !r.dryRun,
        );
        const failures = byNewest.filter((r) => isAuthFailure(r.error));

        const entry: TokenEntry = {
            name: c.name,
            origin: "credential",
            set: c.set,
            inFile: c.inFile,
            declaredBy: c.declaredBy,
            authFailures: failures.length,
        };
        if (success) entry.lastSuccess = runOf(success);
        if (failures[0]) entry.lastAuthFailure = runOf(failures[0]);

        if (!c.set) {
            entry.expiryUnknown = "not set: nothing to read an expiry from";
        } else {
            const value = opts.valueOf(c.name);
            const read = value === undefined ? { unknown: "set, but not readable here" } : jwtExpiry(value);
            if ("atMs" in read) {
                entry.expiry = {
                    atMs: read.atMs,
                    inSeconds: (read.atMs - opts.nowMs) / 1000,
                    source: "jwt",
                };
            } else {
                entry.expiryUnknown = read.unknown;
            }
        }
        return entry;
    });

    const rows = [...entries, ...(opts.rcloneConf ? rcloneExpiries(opts.rcloneConf, opts.nowMs) : [])];

    return {
        entries: rows,
        windowDays,
        runsConsidered: inWindow.length,
        checkedAtMs: opts.nowMs,
    };
}
