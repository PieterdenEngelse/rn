/**
 * Read GitHub's own record of the webhook deliveries it sent, and say which failed.
 *
 * ## Why the sender's log, and why after the fact
 *
 * Monitor → Connection's Listeners board counts every request the hooks door
 * answered. By construction it cannot count one that never reached the socket:
 * the backend restarting, the tunnel down or its mapping removed, the machine
 * asleep. GitHub records those, and does not retry them on its own. Two had
 * already happened on this install before this job existed, and nothing in rn
 * saw either: a 500 ending in `EOF` at 12:52 on 2026-09-08, and a 502 at
 * 14:18:41 on 2026-09-10, eight seconds before a backend restart was logged.
 *
 * Reading the sender's log works where every inside view fails for one reason:
 * it does not need rn to have been up at the moment of the failure, only at
 * some point afterwards. GitHub keeps about three days of deliveries, which is
 * also the window in which a failed one can still be redelivered — so hourly is
 * not impatience, it is what keeps a failure noticeable while it is fixable.
 *
 * ## Decided against: rn probing its own public URL
 *
 * The obvious alternative, and wrong for two measured reasons. The probe runs
 * inside the backend, so during a restart — the one window the 502 above fell
 * in — it is down too. And from this machine it never reaches Funnel at all:
 * MagicDNS resolves the node's `*.ts.net` name to its own tailnet address, where
 * public DNS gives Funnel's relays, so the request would test `tailscale serve`
 * over the tailnet and report "reachable" with Funnel switched off. See
 * `docs/tunnel.md`.
 *
 * ## The id that does not fit in a number
 *
 * GitHub's delivery ids are 64-bit — `3842091948768772096` on a real one — and
 * past 2^53 a JavaScript number cannot hold every integer. `JSON.parse` rounds
 * it without a word, and four consecutive ids collapse to one value. So the ids
 * are not read at all: the cursor is `delivered_at`, and a delivery is known by
 * its `guid`. Redelivering needs the exact id, which is one reason this job
 * reports and does not redeliver.
 *
 * ## What is kept off the record
 *
 * GitHub's status text on a failure can quote the whole request —
 * `POST https://<host>/api/hooks/demo giving up after 1 attempt(s)`. The hook
 * URL is a bearer capability (`docs/token-sec.md`): whoever holds it reaches the
 * listener. The run record is a page, so every URL in that text is replaced
 * before it is stepped, and the hook's configured URL is never read into a step
 * in the first place.
 *
 * The token goes to `api.github.com` and nowhere else. GitHub paginates with a
 * `Link` header naming the next URL, and a next link to any other host is
 * refused rather than followed — following it would send the token along.
 *
 * ## The Node/Rust boundary
 *
 * Node's work by CLAUDE.md's own test: a handful of GETs an hour and some JSON.
 */

import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure, isPermanentStatus } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

const HOST = "api.github.com";
const API = `https://${HOST}`;
/** GitHub refuses an API request with no User-Agent. */
const USER_AGENT = "rn-watch-deliveries (https://github.com/PieterdenEngelse)";
/** Pages of a hundred deliveries followed per webhook before giving up on reaching the cursor. */
const MAX_PAGES = 5;
/** Repositories examined when none are named — one page of GitHub's listing. */
const MAX_REPOS = 100;
/** Characters of GitHub's status text kept per failure. */
const MAX_STATUS = 160;
/**
 * One cursor for every webhook: a map of `owner/repo#hookId` to the newest
 * `delivered_at` seen. One key rather than one per webhook, for the reason
 * `watch-upstreams` gives — a key built from the data grows without bound. At
 * about sixty bytes an entry, MAX_VALUE_BYTES holds some sixty webhooks.
 */
const CURSOR = "delivered";

/** One delivery as GitHub lists it, reduced to what this job reads. No id — see the module doc. */
export interface DeliveryRecord {
    guid: string;
    deliveredAt: string;
    redelivery: boolean;
    statusCode: number;
    status: string;
    event: string;
    action: string | null;
}

/** A failed attempt, and whether a later attempt for the same delivery succeeded. */
export type Failed = DeliveryRecord & { recovered: boolean };

/** GitHub's list, defensively: a field that is missing or the wrong type drops the entry, not the run. */
export function parseDeliveries(raw: unknown): DeliveryRecord[] {
    if (!Array.isArray(raw)) return [];
    const out: DeliveryRecord[] = [];
    for (const item of raw) {
        if (typeof item !== "object" || item === null) continue;
        const r = item as Record<string, unknown>;
        if (typeof r["guid"] !== "string" || typeof r["delivered_at"] !== "string") continue;
        out.push({
            guid: r["guid"],
            deliveredAt: r["delivered_at"],
            redelivery: r["redelivery"] === true,
            statusCode: typeof r["status_code"] === "number" ? r["status_code"] : 0,
            status: typeof r["status"] === "string" ? r["status"] : "",
            event: typeof r["event"] === "string" ? r["event"] : "",
            action: typeof r["action"] === "string" ? r["action"] : null,
        });
    }
    return out;
}

export function succeeded(statusCode: number): boolean {
    return statusCode >= 200 && statusCode < 300;
}

/**
 * GitHub's status text with every URL taken out, whitespace folded, and capped.
 *
 * The character class stops at a quote, so `"https://…/demo":` keeps its
 * quotes and colon and still reads as the sentence it was.
 */
export function scrubStatus(text: string): string {
    return text
        .replace(/https?:\/\/[^\s"'<>]+/gi, "<url>")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, MAX_STATUS);
}

/** `owner/name`, separated by commas or whitespace, with what did not parse kept to be named. */
export function parseRepos(text: string): { repos: string[]; invalid: string[] } {
    const repos: string[] = [];
    const invalid: string[] = [];
    for (const part of text.split(/[\s,]+/).map((p) => p.trim()).filter(Boolean)) {
        if (/^[A-Za-z0-9-]+\/[A-Za-z0-9._-]+$/.test(part)) repos.push(part);
        else invalid.push(part.slice(0, 80));
    }
    return { repos, invalid };
}

/** The `rel="next"` URL in a `Link` header, if there is one. */
export function nextLink(header: string | null): string | undefined {
    if (header === null) return undefined;
    for (const part of header.split(",")) {
        const m = /<([^>]+)>\s*;\s*rel="next"/.exec(part);
        if (m !== null) return m[1];
    }
    return undefined;
}

/**
 * What one webhook's log says, measured against the cursor.
 *
 * `report` is every failed attempt newer than the cursor — or every one still
 * in the log on a first look, or when asked to repeat. **A first look reports,
 * unlike `watch-upstreams`'s**, and the difference is deliberate: an upstream's
 * first look would announce months-old releases, while everything in this log
 * is at most three days old and can still be redelivered. That is news.
 *
 * `recovered` is judged against the whole log fetched, not only the fresh
 * part: a delivery that failed yesterday and was redelivered successfully an
 * hour ago is marked so rather than raised again.
 */
export function assess(
    deliveries: readonly DeliveryRecord[],
    cursor: string | undefined,
    repeat: boolean,
): { fresh: number; report: Failed[]; outstanding: string[]; newest: string | undefined } {
    const since = cursor === undefined ? Number.NaN : Date.parse(cursor);
    const isFresh = (d: DeliveryRecord): boolean =>
        Number.isNaN(since) || Date.parse(d.deliveredAt) > since;

    const answered = new Set(deliveries.filter((d) => succeeded(d.statusCode)).map((d) => d.guid));
    const failed = deliveries.filter((d) => !succeeded(d.statusCode));

    let newest: string | undefined;
    for (const d of deliveries) {
        if (newest === undefined || Date.parse(d.deliveredAt) > Date.parse(newest)) newest = d.deliveredAt;
    }

    const wanted = repeat || Number.isNaN(since) ? failed : failed.filter(isFresh);
    return {
        fresh: deliveries.filter(isFresh).length,
        report: wanted.map((d) => ({ ...d, recovered: answered.has(d.guid) })),
        outstanding: [...new Set(failed.filter((d) => !answered.has(d.guid)).map((d) => d.guid))],
        newest,
    };
}

interface Answer {
    status: number;
    body: unknown;
    next: string | undefined;
}

/**
 * One GET to the API, with the failures sorted by whether asking again helps.
 *
 * 404 and 403 come back as answers rather than errors: GitHub says 404 for "you
 * may not see this repository's webhooks", and a token scoped to some
 * repositories meeting one it cannot read is the ordinary case, not a fault.
 */
async function get(url: string, token: string, signal: AbortSignal): Promise<Answer> {
    let res: Response;
    try {
        res = await fetch(url, {
            headers: {
                accept: "application/vnd.github+json",
                authorization: `Bearer ${token}`,
                "user-agent": USER_AGENT,
                "x-github-api-version": "2022-11-28",
            },
            signal,
        });
    } catch (err) {
        const hint = netPermissionHint(err, [HOST]);
        const message =
            `could not reach ${HOST}: ${err instanceof Error ? err.message : String(err)}` +
            (hint === undefined ? "" : ` — ${hint}`);
        if (hint !== undefined) {
            throw new PermanentFailure(
                message,
                "the runtime's network grant is fixed at startup and cannot widen while it runs",
            );
        }
        throw new Error(message);
    }

    const next = nextLink(res.headers.get("link"));
    if (res.status === 401) {
        throw new PermanentFailure(
            "GitHub refused the token (401) — it is wrong, expired or revoked",
            "a token GitHub does not accept is refused the same way on every attempt",
        );
    }
    if (res.status === 403 && res.headers.get("x-ratelimit-remaining") === "0") {
        // Transient by meaning, and the retry policy's thirty seconds will not
        // outlast an hour's limit — but the next scheduled run will.
        throw new Error("GitHub's API rate limit for this token is used up until the hour turns");
    }
    if (res.status === 404 || res.status === 403) return { status: res.status, body: undefined, next };
    if (!res.ok) {
        const message = `GitHub answered ${res.status} for ${new URL(url).pathname}`;
        if (isPermanentStatus(res.status)) {
            throw new PermanentFailure(message, "GitHub rejected the request itself");
        }
        throw new Error(message);
    }
    return { status: res.status, body: await res.json(), next };
}

/** `full_name` of each repository in GitHub's listing. */
function fullNames(body: unknown): string[] {
    if (!Array.isArray(body)) return [];
    return body
        .map((r) => (typeof r === "object" && r !== null ? (r as Record<string, unknown>)["full_name"] : undefined))
        .filter((n): n is string => typeof n === "string");
}

/** The webhooks on one repository — id, whether active, which events. Never the URL. */
function webhooksIn(body: unknown): { id: string; active: boolean; events: string[] }[] {
    if (!Array.isArray(body)) return [];
    const out: { id: string; active: boolean; events: string[] }[] = [];
    for (const item of body) {
        if (typeof item !== "object" || item === null) continue;
        const h = item as Record<string, unknown>;
        // Hook ids are well inside 2^53, unlike delivery ids.
        if (typeof h["id"] !== "number") continue;
        out.push({
            id: String(h["id"]),
            active: h["active"] === true,
            events: Array.isArray(h["events"]) ? h["events"].filter((e): e is string => typeof e === "string") : [],
        });
    }
    return out;
}

/**
 * One webhook's deliveries, newest first, back to the cursor.
 *
 * GitHub lists newest first — checked against the real log, where every id and
 * every timestamp descends — so once a page reaches back past the cursor, the
 * rest is already known and is not fetched. `undefined` means the token may not
 * read this webhook's deliveries.
 */
async function deliveriesOf(
    repo: string,
    hookId: string,
    cursor: string | undefined,
    token: string,
    ctx: JobContext,
): Promise<DeliveryRecord[] | undefined> {
    const since = cursor === undefined ? Number.NaN : Date.parse(cursor);
    const all: DeliveryRecord[] = [];
    let url: string | undefined = `${API}/repos/${repo}/hooks/${hookId}/deliveries?per_page=100`;
    for (let page = 0; url !== undefined; page += 1) {
        if (page === MAX_PAGES) {
            ctx.step("deliveries-truncated", { repo, hook: hookId, pages: MAX_PAGES });
            break;
        }
        const res: Answer = await get(url, token, ctx.signal);
        if (res.status !== 200) return undefined;
        const batch = parseDeliveries(res.body);
        all.push(...batch);
        const oldest = batch.at(-1);
        if (oldest === undefined) break;
        if (!Number.isNaN(since) && Date.parse(oldest.deliveredAt) <= since) break;

        url = res.next;
        if (url !== undefined && new URL(url).host !== HOST) {
            ctx.step("pagination-refused", {
                host: new URL(url).host,
                effect: "the token is sent to api.github.com only; the rest of this log was not read",
            });
            break;
        }
    }
    return all;
}

export const watchDeliveries: Job = {
    id: "watch-deliveries",
    label: "Watch webhook deliveries",

    // Hourly, because the log it reads is about three days long and the
    // scheduler does not catch up on slots missed while the laptop slept: an
    // hourly job gets dozens of chances inside the window, a daily one three.
    schedule: { kind: "everyMinutes", minutes: 60 },

    source: import.meta.filename,

    // Every request is a GET, and the only thing written is the cursor. So a
    // disarmed install still remembers what it has reported, rather than
    // raising the same failures every hour. See Job.effectFree.
    effectFree: true,

    // A few small requests per webhook. A minute bounds the one case worth
    // bounding: an API that accepts the connection and never answers.
    timeoutMs: 60_000,

    // GitHub's API has bad minutes and a laptop's wifi wakes late; both are
    // what asking again is for. A refused token is thrown as permanent and
    // skips the retries.
    retry: { attempts: 3, backoffMs: 30_000 },

    // The whole point is being told. A failure nobody sees until they open a
    // page is the state this job exists to end, so a run that finds one hands
    // it to the desktop — overridable on Config → Jobs like any handoff.
    onChange: "desktop-notify",

    credentials: ["githubToken"],

    probes: {
        // The cheapest authenticated call GitHub has: /rate_limit is itself
        // exempt from the rate limit, so asking whether the token works cannot
        // cost the budget the token is for. A 401 here is the answer the board
        // exists to give — the same refusal that would otherwise surface at
        // 04:00 as a failed run with nothing naming the cause.
        githubToken: async (ctx) => {
            const token = ctx.secret("githubToken");
            if (token === undefined) return { ok: false, detail: "not set" };
            const res = await fetch("https://api.github.com/rate_limit", {
                headers: {
                    authorization: `Bearer ${token}`,
                    accept: "application/vnd.github+json",
                    "user-agent": "rn",
                },
                signal: AbortSignal.timeout(10_000),
            });
            if (!res.ok) {
                return { ok: false, detail: `${res.status} ${res.statusText}` };
            }
            const body = (await res.json()) as { rate?: { remaining?: number; limit?: number } };
            const rate = body.rate;
            return {
                ok: true,
                detail:
                    rate === undefined
                        ? "accepted"
                        : `accepted · ${rate.remaining ?? "?"}/${rate.limit ?? "?"} left this hour`,
            };
        },
    },

    inputs: [
        {
            id: "repos",
            label: "Repositories",
            type: "text",
            default: "",
            placeholder: "empty — every repository the token can see",
            info: {
                what:
                    "Which repositories to check, as owner/name separated by commas. Empty — the " +
                    "default — means every repository the token can see, which for a " +
                    "fine-grained token is exactly the ones you granted it.",
                why:
                    "Empty follows the token: grant it another repository and that repository is " +
                    "watched from the next run, with nothing to edit here. Naming them is for a " +
                    "token that sees far more than you want read every hour.",
                ifWrong:
                    "A name that is not owner/name is listed as invalid in the steps and skipped. " +
                    "A repository the token cannot read shows as no-access. Leave one out and its " +
                    "failures are never reported, with nothing failing to say so.",
            },
        },
        {
            id: "repeat",
            label: "Report every failure still in the log",
            type: "bool",
            default: false,
            info: {
                what:
                    "Off, a run reports only failures since the last run. On, it reports every " +
                    "failed delivery still in GitHub's log — about the last three days — whether " +
                    "or not you have already been told.",
                why:
                    "Off is the job: an hourly report that repeats itself is one you mute. On is " +
                    "for sitting down to redeliver, when you want the whole standing list at once.",
                ifWrong:
                    "Left on, every hourly run raises the same failures again — and with on-change " +
                    "handing to desktop-notify, the same notification every hour until they age " +
                    "out of GitHub's log.",
            },
        },
    ],

    info: {
        what:
            "Asks GitHub, once an hour, for its own record of every webhook delivery it sent to " +
            "the repositories this token can see — about three days of them — and reports each " +
            "attempt that did not get a 2xx: when, which event, the status code, and GitHub's " +
            "reason. It remembers the newest delivery it has seen on each webhook, so the next " +
            "run reports only failures since; a failure that was later redelivered successfully " +
            "is marked recovered rather than raised.\n\nEvery request is a GET to api.github.com, " +
            "and nothing is redelivered or changed. GitHub's reason text often quotes the webhook " +
            "URL; every URL in it is removed before anything is recorded, because that URL is " +
            "the capability that reaches this machine.",
        why:
            "The Listeners board on Monitor → Connection counts what reached rn. A delivery that " +
            "never did — rn restarting, the tunnel down, the laptop asleep — is invisible from " +
            "inside rn by construction, and GitHub does not retry it. It happened twice in three " +
            "days on this install before this job existed: a 500 on 2026-09-08 and a 502 during " +
            "a backend restart on 2026-09-10, and nothing in rn saw either.\n\nThe sender's log " +
            "is the only record, and GitHub keeps it — and lets a delivery be redelivered from " +
            "it — for about three days. Reading it hourly is how a failure gets noticed while it " +
            "can still be fixed. It reads the log rather than probing rn's own public URL, " +
            "because a probe runs inside the backend and is down whenever rn is.",
        ifWrong:
            "It needs a GitHub token named githubToken — RN_SECRET_GITHUB_TOKEN in " +
            "~/.config/rn/credentials. A fine-grained token needs the Webhooks repository " +
            "permission, read-only, on each repository to watch; a classic token needs " +
            "read:repo_hook. A token that cannot read one repository's webhooks makes that " +
            "repository show as no-access in the steps rather than failing the run; a token " +
            "GitHub rejects fails the run at once, without retrying.\n\nThe log is about three " +
            "days long, so if rn does not run for longer than that, failures that aged out are " +
            "gone and the next run cannot know they happened. Under Deno, api.github.com must " +
            "be on the outbound allowlist on Config → Connection.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const token = ctx.secret("githubToken");
        const repeat = ctx.input.repeat === true;
        const named = parseRepos(String(ctx.input.repos ?? ""));
        if (named.invalid.length > 0) {
            ctx.step("repo-invalid", { names: named.invalid, expected: "owner/name" });
        }
        if (named.repos.length === 0 && named.invalid.length > 0) {
            return {
                summary: {},
                changed: false,
                skipped: "No repository to check — every name given was invalid (listed in the steps).",
            };
        }

        let repos = named.repos;
        let from = "named";
        if (repos.length === 0) {
            from = "token";
            const res = await get(`${API}/user/repos?per_page=${MAX_REPOS}&sort=full_name`, token, ctx.signal);
            repos = res.status === 200 ? fullNames(res.body) : [];
            if (repos.length >= MAX_REPOS) {
                ctx.step("repos-truncated", {
                    kept: MAX_REPOS,
                    effect: "repositories past the first hundred were not checked — name the ones to watch",
                });
            }
        }
        ctx.step("repos", { count: repos.length, from });
        if (repos.length === 0) {
            return { summary: { repos: 0 }, changed: false, skipped: "The token can see no repository." };
        }

        const held = ctx.state.get(CURSOR);
        const remembered: Record<string, string> =
            typeof held === "object" && held !== null && !Array.isArray(held)
                ? Object.fromEntries(
                      Object.entries(held).filter((e): e is [string, string] => typeof e[1] === "string"),
                  )
                : {};
        const next: Record<string, string> = {};

        let hooks = 0;
        let examined = 0;
        let sinceLast = 0;
        let firstLook = 0;
        let noAccess = 0;
        const reported: (Failed & { repo: string })[] = [];
        const outstanding = new Set<string>();

        for (const repo of repos) {
            const listed = await get(`${API}/repos/${repo}/hooks?per_page=100`, token, ctx.signal);
            if (listed.status !== 200) {
                noAccess += 1;
                ctx.step("no-access", { repo, status: listed.status });
                continue;
            }
            for (const hook of webhooksIn(listed.body)) {
                hooks += 1;
                const key = `${repo}#${hook.id}`;
                const cursor = remembered[key];
                const log = await deliveriesOf(repo, hook.id, cursor, token, ctx);
                if (log === undefined) {
                    noAccess += 1;
                    ctx.step("no-access", { repo, hook: hook.id, what: "deliveries" });
                    // Kept rather than dropped, so regaining access does not
                    // turn into a first look that re-reports three days.
                    if (cursor !== undefined) next[key] = cursor;
                    continue;
                }

                const a = assess(log, cursor, repeat);
                examined += log.length;
                sinceLast += a.fresh;
                if (cursor === undefined) firstLook += 1;
                const newest = a.newest ?? cursor;
                if (newest !== undefined) next[key] = newest;

                ctx.step("webhook", {
                    repo,
                    hook: hook.id,
                    active: hook.active,
                    events: hook.events.join(","),
                    deliveries: log.length,
                    sinceLast: a.fresh,
                    ...(cursor === undefined ? { firstLook: true } : {}),
                });
                for (const f of a.report) {
                    reported.push({ ...f, repo });
                    ctx.step("failed", {
                        repo,
                        hook: hook.id,
                        at: f.deliveredAt,
                        code: f.statusCode,
                        reason: scrubStatus(f.status),
                        event: f.event,
                        ...(f.action === null ? {} : { action: f.action }),
                        guid: f.guid,
                        ...(f.redelivery ? { redelivery: true } : {}),
                        recovered: f.recovered,
                    });
                }
                for (const g of a.outstanding) outstanding.add(`${key}:${g}`);
            }
        }

        // Staged, not written — the runner commits after run() returns. Only
        // webhooks that still exist are kept, so the map cannot outgrow them.
        if (Object.keys(next).length > 0) ctx.state.changed(CURSOR, next);

        const open = reported.filter((f) => !f.recovered);
        const latest = open[0] ?? reported[0];
        const summary = {
            repos: repos.length,
            webhooks: hooks,
            examined,
            sinceLast,
            failures: reported.length,
            outstanding: outstanding.size,
            ...(noAccess === 0 ? {} : { noAccess }),
            ...(firstLook === 0 ? {} : { firstLook }),
            // One line a notification can carry, since desktop-notify's body is
            // this summary: which delivery, what it got, when.
            ...(latest === undefined
                ? {}
                : { latestFailure: `${latest.repo} ${latest.event} → ${latest.statusCode} at ${latest.deliveredAt}` }),
        };

        if (hooks === 0) {
            return {
                summary,
                changed: false,
                skipped:
                    noAccess > 0
                        ? `No webhook this token can read: ${noAccess} of ${repos.length} repositories ` +
                          `refused. A fine-grained token needs the Webhooks permission (read-only) on ` +
                          `each; a classic one needs read:repo_hook.`
                        : `None of the ${repos.length} repositories has a webhook.`,
            };
        }
        if (reported.length === 0) {
            // Two different quiet runs, and the difference is worth saying:
            // "nothing failed" is not true while an earlier failure sits in the
            // log unredelivered. It was reported when it happened; this names
            // that it is still there without raising it again.
            return {
                summary,
                changed: false,
                skipped:
                    outstanding.size === 0
                        ? `Nothing failed: ${examined} deliveries examined across ${hooks} ` +
                          `webhook(s), ${sinceLast} of them since the last run.`
                        : `No new failure: ${examined} deliveries examined across ${hooks} ` +
                          `webhook(s), ${sinceLast} since the last run. ${outstanding.size} earlier ` +
                          `failure(s) still not redelivered — turn on "Report every failure still ` +
                          `in the log" to list them again.`,
            };
        }
        if (open.length === 0) {
            return {
                summary,
                changed: false,
                skipped:
                    `${reported.length} failed attempt(s), every one since redelivered successfully ` +
                    `— listed in the steps.`,
            };
        }
        if (ctx.dryRun) {
            // Same report either way, and the cursor moved (effectFree). What
            // dry run withholds is `changed`, the handoff to desktop-notify.
            return {
                summary,
                changed: false,
                skipped:
                    `${open.length} failed deliver${open.length === 1 ? "y" : "ies"} not since ` +
                    `redelivered, listed in the steps. DRY_RUN is on, so nothing is handed to ` +
                    `desktop-notify — arm rn on Config → Runtime for that.`,
            };
        }
        return { summary, changed: true };
    },
};
