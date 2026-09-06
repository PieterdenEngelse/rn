/**
 * Minted links, and the clicks that came back.
 *
 * The only store in rn that grows with traffic from outside the machine, which
 * is what makes it different from every other file beside it. `job-state.json`
 * is a bounded window somebody's own automation writes; this one is appended to
 * by strangers, at a rate set by how many people were mailed.
 *
 * ## Why the id has to be looked up rather than decoded
 *
 * A tracked link could carry its destination — signed, so it could not be
 * tampered with — and then no store would be needed at all. That design is an
 * open redirect on a public HTTPS host carrying this machine's name: anyone who
 * can mint or guess a link can send a stranger anywhere, with rn's hostname on
 * the hop. So the destination is *never* in the URL. `/t/<id>` is opaque, the
 * store says where it goes, and a request for an id nobody minted is a 404.
 *
 * See docs/link-tracking.md §3.
 *
 * ## What identity is, and what it deliberately is not
 *
 * `recipient` is nullable and that is the whole of §6's remaining decision.
 * `null` is a per-send link — one id shared by everyone the mail went to, which
 * answers *did this land* and nothing more. A value is an identified link,
 * minted for one person.
 *
 * **Ids are random and scoped to one send.** Never derived from the address,
 * never stable across sends. That is not a detail: a stable per-recipient id
 * lets anyone who collects links from two mailings build a profile, and the
 * random per-send id removes that entirely while keeping *Alice clicked in send
 * 12*. It is the one of §6's four costs that is fully fixable, and this is
 * where it is fixed.
 *
 * ## Retention, which is a decision and not a default
 *
 * Three lifetimes, deliberately different:
 *
 * - **Links never expire.** A link lives in somebody's mailbox and may be
 *   clicked years later; expiring the id turns that into a 404 in mail a person
 *   kept. Losing analytics is an annoyance, breaking a link somebody was sent
 *   is a fault.
 * - **Identity expires** after `trackerRetentionDays`. The `recipient` is
 *   dropped and the link goes on resolving, so what rn remembers about a person
 *   is bounded while the mail keeps working.
 * - **Clicks expire** on the same clock, because a click is the record of a
 *   person's behaviour and there is no reason for it to outlive the identity it
 *   was attached to.
 *
 * **What retention does not fix**, stated here because the docstring is where
 * somebody will look for reassurance: the distinct hrefs are already in
 * mailboxes. Two recipients comparing their copies still learn the mail was
 * individually tracked, and a link pasted into a chat still produces clicks
 * attributed to whoever it was minted for, long after this file has forgotten
 * who that was. Retention bounds the store, not the artifact.
 *
 * ## The file
 *
 * JSONL, one record per line, appended. A crash mid-write costs the last line
 * rather than the file — which matters more here than for the JSON stores
 * beside it, because those are rewritten wholly by a person pressing save and
 * this one is written by a request handler. Compaction rewrites it through a
 * temporary file and a rename, so a reader never sees a half-file.
 */

import { appendFileSync, existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { step, warn } from "../log.ts";

/** A link that was minted. Never removed — see the retention note above. */
export interface Link {
    id: string;
    sendId: string;
    /** Null for a per-send link. Dropped once identity ages out. */
    recipient: string | null;
    url: string;
    mintedAt: number;
}

/** One arrival at `/t/<id>`. */
export interface Click {
    id: string;
    at: number;
    /**
     * Kept because it is the only thing that distinguishes a scanner from a
     * person, and even then only sometimes — see docs/link-tracking.md §5. It
     * is evidence to show, not a filter to hide behind.
     */
    userAgent: string;
    /**
     * `GET` or `HEAD`.
     *
     * Recorded rather than filtered on, which is the same rule the user-agent
     * follows. No browser navigates with HEAD, so a HEAD is a link checker or a
     * scanner and never a person — but dropping it here would hide the
     * strongest single piece of evidence §5 has, and answering 404 to it would
     * make the link look broken to the checker. So it redirects, it is
     * recorded, and the page can say what it was.
     */
    method: string;
}

type Record_ =
    | ({ t: "link" } & Link)
    | ({ t: "click" } & Click);

/** id → link. Rebuilt from the file at load; the file is the truth. */
let links = new Map<string, Link>();
let clicks: Click[] = [];
let loaded = false;
let lastPrune = 0;

/** An hour. Pruning walks every record, so it is not done per request. */
const PRUNE_INTERVAL_MS = 60 * 60 * 1000;

function path(): string {
    return config.trackerStorePath;
}

function retentionMs(): number {
    return config.trackerRetentionDays * 24 * 60 * 60 * 1000;
}

/**
 * Read the file into memory.
 *
 * A malformed line is skipped and counted rather than thrown on. The file is
 * appended to by a request handler, so the realistic corruption is one
 * truncated last line after a hard kill — and refusing to start because of it
 * would take every working link down with it.
 */
export function load(): void {
    links = new Map();
    clicks = [];
    loaded = true;
    if (!existsSync(path())) return;

    let skipped = 0;
    for (const line of readFileSync(path(), "utf8").split("\n")) {
        if (line.trim() === "") continue;
        let rec: Record_;
        try {
            rec = JSON.parse(line) as Record_;
        } catch {
            skipped += 1;
            continue;
        }
        if (rec.t === "link") links.set(rec.id, rec);
        else if (rec.t === "click") clicks.push(rec);
        else skipped += 1;
    }
    if (skipped > 0) warn("tracker-store-skipped-lines", { lines: skipped, path: path() });
    step("tracker-store-loaded", { links: links.size, clicks: clicks.length });
    prune();
}

function ensureLoaded(): void {
    if (!loaded) load();
}

function append(rec: Record_): void {
    mkdirSync(dirname(path()), { recursive: true });
    appendFileSync(path(), `${JSON.stringify(rec)}\n`, "utf8");
}

/**
 * Mint a link for one URL.
 *
 * `recipient` null makes a per-send link. The id is 96 bits of randomness in
 * base64url — opaque, unguessable, and carrying nothing about the address it
 * was minted for.
 */
export function mint(sendId: string, recipient: string | null, url: string): Link {
    ensureLoaded();
    const link: Link = {
        id: randomBytes(12).toString("base64url"),
        sendId,
        recipient,
        url,
        mintedAt: Date.now(),
    };
    links.set(link.id, link);
    append({ t: "link", ...link });
    maybePrune();
    return link;
}

/** The link an id names, or undefined. Undefined is a 404, never a guess. */
export function resolve(id: string): Link | undefined {
    ensureLoaded();
    return links.get(id);
}

/**
 * Record an arrival.
 *
 * Returns false for an id nothing minted, so the caller answers 404 without
 * having to ask twice. Nothing about the *reason* reaches the response: an
 * expired identity and an unminted id look identical from outside, because a
 * caller who could tell them apart could enumerate what has been sent.
 */
export function click(id: string, userAgent: string, method = "GET"): boolean {
    ensureLoaded();
    if (!links.has(id)) return false;
    const rec: Click = { id, at: Date.now(), userAgent: userAgent.slice(0, 512), method };
    clicks.push(rec);
    append({ t: "click", ...rec });
    maybePrune();
    return true;
}

/** Every click on a link, newest last. */
export function clicksFor(id: string): Click[] {
    ensureLoaded();
    return clicks.filter((c) => c.id === id);
}

/** Every link in a send, in the order they were minted. */
export function linksFor(sendId: string): Link[] {
    ensureLoaded();
    return [...links.values()].filter((l) => l.sendId === sendId);
}

/** Every send id the store knows, newest first. */
export function sends(): string[] {
    ensureLoaded();
    const seen = new Map<string, number>();
    for (const l of links.values()) {
        const at = seen.get(l.sendId);
        if (at === undefined || l.mintedAt > at) seen.set(l.sendId, l.mintedAt);
    }
    return [...seen.entries()].sort((a, b) => b[1] - a[1]).map(([id]) => id);
}

function maybePrune(): void {
    if (Date.now() - lastPrune < PRUNE_INTERVAL_MS) return;
    prune();
}

/**
 * Drop what has aged out, and rewrite the file if anything did.
 *
 * Exported so a test can run it without waiting an hour, and so the tracker can
 * be pruned on demand. Links survive; identity and clicks do not.
 */
export function prune(): number {
    lastPrune = Date.now();
    const cutoff = Date.now() - retentionMs();
    let dropped = 0;

    for (const [id, link] of links) {
        if (link.recipient !== null && link.mintedAt < cutoff) {
            links.set(id, { ...link, recipient: null });
            dropped += 1;
        }
    }
    const keptClicks = clicks.filter((c) => c.at >= cutoff);
    dropped += clicks.length - keptClicks.length;
    clicks = keptClicks;

    if (dropped > 0) {
        compact();
        step("tracker-store-pruned", { dropped, retentionDays: config.trackerRetentionDays });
    }
    return dropped;
}

/** Rewrite the file from memory, through a temporary file and a rename. */
function compact(): void {
    mkdirSync(dirname(path()), { recursive: true });
    const lines: string[] = [];
    for (const link of links.values()) lines.push(JSON.stringify({ t: "link", ...link }));
    for (const c of clicks) lines.push(JSON.stringify({ t: "click", ...c }));
    const tmp = `${path()}.tmp`;
    writeFileSync(tmp, lines.length === 0 ? "" : `${lines.join("\n")}\n`, "utf8");
    renameSync(tmp, path());
}

/** Drop everything in memory so the next call re-reads. For tests. */
export function reset(): void {
    loaded = false;
    lastPrune = 0;
}
