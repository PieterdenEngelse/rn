/**
 * The pages this install watches, and the file they live in.
 *
 * Modelled on `mail/rules.ts`, which is modelled on `webhooks.ts`: its own JSON
 * file, validated on the way in, rewritten whole on save. Not `settings.json`,
 * which holds scalar runtime parameters and has no shape for a list of records.
 *
 * ## Why this replaced two settings
 *
 * `watch-pages` shipped with `RN_WATCH_PAGES` and `RN_WATCH_PAGES_IGNORE`, two
 * comma-separated strings. They could say *which* pages and *what noise to
 * drop*, but only as one answer for every page at once — and the job's own
 * panel had to admit it: "a word that is noise on one page and content on
 * another wants a second run of this job". A record per page makes the ignore
 * list, the comparison mode and the cadence properties of the page they belong
 * to, which is what they always were.
 *
 * ## What is not in here
 *
 * When each page was last fetched, and what it looked like. Those live in the
 * job's own state, for the reason `webhooks.ts` keeps its delivery counters out
 * of its definitions file: a value written on every run means rewriting a file
 * somebody typed, from a code path that has no business touching it. This file
 * changes when a person changes it, and never otherwise.
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname } from "node:path";
import { config } from "./config.ts";
import { debug, step, warn } from "./log.ts";
import type { WatchedPage } from "./generated/wire.ts";

/**
 * The cadence a page gets when it asks for none.
 *
 * An hour, which is what the job's schedule was when every page shared one.
 */
export const DEFAULT_EVERY_MINUTES = 60;

/** Below this, a page is asking for more often than any job can deliver. */
const MIN_EVERY_MINUTES = 1;

/** A page asking for less often than a fortnight is asking to be forgotten. */
const MAX_EVERY_MINUTES = 20_160;

let pages: WatchedPage[] = [];
let loaded = false;

function path(): string {
    return config.watchPagesPath;
}

/** Everything a stored record must be before it is believed. */
function sane(raw: unknown): WatchedPage | undefined {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return undefined;
    const p = raw as Record<string, unknown>;
    if (typeof p["id"] !== "string" || p["id"] === "") return undefined;
    if (typeof p["url"] !== "string" || p["url"].trim() === "") return undefined;
    const every = typeof p["everyMinutes"] === "number" ? p["everyMinutes"] : DEFAULT_EVERY_MINUTES;
    return {
        id: p["id"],
        url: p["url"].trim(),
        label: typeof p["label"] === "string" ? p["label"] : "",
        // Absent means on. A record written by an older version, or by hand,
        // should be watched rather than silently ignored — the same default
        // `mail/rules.ts` takes, and for the same reason.
        enabled: p["enabled"] !== false,
        ignore: typeof p["ignore"] === "string" ? p["ignore"] : "",
        text: p["text"] !== false,
        everyMinutes: Math.min(MAX_EVERY_MINUTES, Math.max(MIN_EVERY_MINUTES, Math.trunc(every))),
        createdAt: typeof p["createdAt"] === "number" ? p["createdAt"] : Date.now(),
    };
}

export function load(): void {
    pages = [];
    loaded = true;
    let raw: string;
    try {
        raw = readFileSync(path(), "utf8");
    } catch {
        return;
    }
    try {
        const parsed: unknown = JSON.parse(raw);
        if (!Array.isArray(parsed)) {
            warn("watch-pages-unreadable", {
                path: path(),
                reason: "the file is not a list",
                effect: "no page is watched",
            });
            return;
        }
        let dropped = 0;
        for (const entry of parsed) {
            const p = sane(entry);
            if (p === undefined) dropped += 1;
            else pages.push(p);
        }
        debug("watch-pages-loaded", { count: pages.length, dropped });
    } catch (err) {
        // Named rather than swallowed: a file that fails to parse means every
        // page is silently unwatched, which is the failure this feature is
        // least able to notice on its own — an unwatched page looks exactly
        // like a page that has not changed.
        warn("watch-pages-unreadable", {
            path: path(),
            reason: err instanceof Error ? err.message : String(err),
            effect: "no page is watched",
        });
    }
}

function ensureLoaded(): void {
    if (!loaded) load();
}

function persist(): void {
    mkdirSync(dirname(path()), { recursive: true });
    writeFileSync(path(), `${JSON.stringify(pages, null, 2)}\n`, "utf8");
}

/** Every page, in the order they were made. */
export function list(): readonly WatchedPage[] {
    ensureLoaded();
    return pages;
}

/** The file these live in, for a page that wants to say where its data is. */
export function storePath(): string {
    return path();
}

/**
 * What is wrong with a record, in the order somebody would fix it.
 *
 * A URL is refused here rather than at 04:00. The whole point of validating on
 * the way in is that a mistake is answered while the person who made it is
 * looking at it, instead of becoming a step in a run nobody reads.
 */
export function validate(raw: unknown): { errors: string[]; page?: WatchedPage } {
    const errors: string[] = [];
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
        return { errors: ["the body is not an object"] };
    }
    const p = raw as Record<string, unknown>;

    const url = typeof p["url"] === "string" ? p["url"].trim() : "";
    if (url === "") {
        errors.push("give it a URL — that is the whole subject of the record");
    } else {
        let parsed: URL | undefined;
        try {
            parsed = new URL(url);
        } catch {
            parsed = undefined;
        }
        if (parsed === undefined) {
            errors.push(`"${url.slice(0, 80)}" is not a URL`);
        } else if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
            // Not tidiness: a file: URL would turn this field into a way to
            // read the disk of the machine rn runs on, through a page.
            errors.push("http and https only — no other scheme is fetched");
        }
    }

    const every =
        typeof p["everyMinutes"] === "number" ? Math.trunc(p["everyMinutes"]) : DEFAULT_EVERY_MINUTES;
    if (!Number.isFinite(every) || every < MIN_EVERY_MINUTES) {
        errors.push(`check no more often than every ${MIN_EVERY_MINUTES} minute(s)`);
    }
    if (every > MAX_EVERY_MINUTES) {
        errors.push(`check at least every ${MAX_EVERY_MINUTES} minutes — beyond that, delete it`);
    }

    if (errors.length > 0) return { errors };

    const id = typeof p["id"] === "string" && p["id"] !== "" ? p["id"] : randomBytes(8).toString("hex");
    return {
        errors: [],
        page: {
            id,
            url,
            label: typeof p["label"] === "string" ? p["label"].trim() : "",
            enabled: p["enabled"] !== false,
            ignore: typeof p["ignore"] === "string" ? p["ignore"].trim() : "",
            text: p["text"] !== false,
            everyMinutes: every,
            createdAt: typeof p["createdAt"] === "number" ? p["createdAt"] : Date.now(),
        },
    };
}

/** Add or replace one record. The id in the body decides which. */
export function put(raw: unknown): { errors: string[]; page?: WatchedPage } {
    ensureLoaded();
    const result = validate(raw);
    if (result.page === undefined) return result;

    const at = pages.findIndex((p) => p.id === result.page?.id);
    if (at === -1) pages.push(result.page);
    else pages[at] = result.page;
    persist();
    step("watch-page-saved", {
        id: result.page.id,
        url: result.page.url,
        enabled: result.page.enabled,
        everyMinutes: result.page.everyMinutes,
        replacing: at === -1 ? null : result.page.id,
    });
    return result;
}

/**
 * Remove one record. False when there was none with that id.
 *
 * What the job remembers about the page is *not* removed here, and that is
 * deliberate rather than an oversight: the job prunes marks for pages that are
 * no longer in the list, so a record deleted by accident and re-added within
 * the same run window comes back knowing where it stood.
 */
export function remove(id: string): boolean {
    ensureLoaded();
    const at = pages.findIndex((p) => p.id === id);
    if (at === -1) return false;
    const [gone] = pages.splice(at, 1);
    persist();
    step("watch-page-removed", { id, url: gone?.url ?? "" });
    return true;
}

/** Drop everything in memory. Tests only. */
export function reset(): void {
    pages = [];
    loaded = false;
}
