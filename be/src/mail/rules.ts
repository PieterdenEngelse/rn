/**
 * The mail rules an install has made, and the file they live in.
 *
 * Modelled on `webhooks.ts`, which is the precedent for a list a page can edit:
 * its own JSON file, validated on the way in, rewritten whole on save. Not
 * `settings.json`, which holds scalar runtime parameters and has no shape for a
 * list of records.
 *
 * ## Rules for one mailbox are ORed; fields inside a rule are ANDed
 *
 * That combination is the whole reason this replaced three install-wide
 * settings. `from: her` and `to: her` as global values cannot both hold for one
 * message, so "when she writes to me, or when I write to her" was unsayable —
 * two rules say it without effort. And keeping the fields within a rule ANDed
 * is what preserves "from me *to* her" as a single precise statement rather
 * than a pair of loose ones.
 *
 * ## Nothing here decides what is fetched
 *
 * A rule is data. `read-mail` turns the rules for one mailbox into an IMAP
 * search and then rechecks the parsed addresses, and `watcher.ts` reads the
 * distinct mailboxes to know what to hold connections for. Keeping the
 * matching out of this module is deliberate: the address comparison is the part
 * with a spoofing hazard in it, and it lives in one place, tested.
 */

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { randomBytes } from "node:crypto";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { debug, step, warn } from "../log.ts";
import { parseSenders } from "./extract.ts";
import type { MailRule } from "../generated/wire.ts";

let rules: MailRule[] = [];
let loaded = false;

function path(): string {
    return config.mailRulesPath;
}

/** Everything a stored rule must be before it is believed. */
function sane(raw: unknown): MailRule | undefined {
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return undefined;
    const r = raw as Record<string, unknown>;
    if (typeof r["id"] !== "string" || r["id"] === "") return undefined;
    if (typeof r["mailbox"] !== "string" || r["mailbox"].trim() === "") return undefined;
    return {
        id: r["id"],
        mailbox: r["mailbox"].trim(),
        from: typeof r["from"] === "string" ? r["from"] : "",
        to: typeof r["to"] === "string" ? r["to"] : "",
        enabled: r["enabled"] !== false,
        label: typeof r["label"] === "string" ? r["label"] : "",
    };
}

export function load(): void {
    rules = [];
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
            warn("mail-rules-unreadable", {
                path: path(),
                reason: "the file is not a list",
                effect: "no mail rules are in force",
            });
            return;
        }
        let dropped = 0;
        for (const entry of parsed) {
            const r = sane(entry);
            if (r === undefined) dropped += 1;
            else rules.push(r);
        }
        debug("mail-rules-loaded", { count: rules.length, dropped });
    } catch (err) {
        // Named rather than swallowed. A rule file that fails to parse means
        // every rule is silently off, which is the failure this feature is
        // least able to notice on its own.
        warn("mail-rules-unreadable", {
            path: path(),
            reason: err instanceof Error ? err.message : String(err),
            effect: "no mail rules are in force",
        });
    }
}

function ensureLoaded(): void {
    if (!loaded) load();
}

function persist(): void {
    mkdirSync(dirname(path()), { recursive: true });
    writeFileSync(path(), `${JSON.stringify(rules, null, 2)}\n`, "utf8");
}

/** Every rule, in the order they were made. */
export function list(): readonly MailRule[] {
    ensureLoaded();
    return rules;
}

/** Enabled rules for one mailbox. What `read-mail` actually applies. */
export function forMailbox(mailbox: string): MailRule[] {
    ensureLoaded();
    return rules.filter((r) => r.enabled && r.mailbox === mailbox);
}

/** Distinct mailboxes any enabled rule names. What the watcher connects to. */
export function watchedMailboxes(): string[] {
    ensureLoaded();
    const out: string[] = [];
    for (const r of rules) {
        if (r.enabled && !out.includes(r.mailbox)) out.push(r.mailbox);
    }
    return out;
}

/**
 * What is wrong with a rule, in the order somebody would fix it.
 *
 * A rule naming neither a sender nor a recipient is refused rather than treated
 * as "everything in this mailbox". That is a defensible thing to want and a
 * terrible thing to arrive at by leaving two boxes empty: the run would report
 * every message in the mailbox onto a page and into the job history. Say it
 * with `*` if you mean it.
 */
export function validate(raw: unknown): { errors: string[]; rule?: MailRule } {
    const errors: string[] = [];
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
        return { errors: ["a rule must be an object"] };
    }
    const r = raw as Record<string, unknown>;

    const mailbox = typeof r["mailbox"] === "string" ? r["mailbox"].trim() : "";
    if (mailbox === "") errors.push("mailbox is required — INBOX, or a label as the server spells it");

    const from = typeof r["from"] === "string" ? r["from"].trim() : "";
    const to = typeof r["to"] === "string" ? r["to"].trim() : "";
    if (from === "" && to === "") {
        errors.push(
            "name a sender or a recipient — a rule matching everything in a mailbox " +
                'puts every message in it on a page and in the history. Write "*" in one ' +
                "of them if that is genuinely what you want",
        );
    }

    for (const [field, value] of [
        ["from", from],
        ["to", to],
    ] as const) {
        if (value === "" || value === "*") continue;
        for (const pattern of parseSenders(value)) {
            // A pattern with no dot cannot be an address or a domain, and the
            // most likely cause is somebody typing a person's name.
            if (!pattern.includes(".")) {
                errors.push(`${field}: "${pattern}" is not an address or a domain`);
            }
        }
    }

    if (errors.length > 0) return { errors };

    return {
        errors: [],
        rule: {
            id: typeof r["id"] === "string" && r["id"] !== "" ? r["id"] : randomBytes(8).toString("hex"),
            mailbox,
            from: from === "*" ? "" : from,
            to: to === "*" ? "" : to,
            enabled: r["enabled"] !== false,
            label: typeof r["label"] === "string" ? r["label"].trim() : "",
        },
    };
}

/** Add or replace one rule. Returns what was stored, or why it was not. */
export function put(raw: unknown): { errors: string[]; rule?: MailRule } {
    ensureLoaded();
    const result = validate(raw);
    if (result.rule === undefined) return result;

    const at = rules.findIndex((r) => r.id === result.rule?.id);
    if (at === -1) rules.push(result.rule);
    else rules[at] = result.rule;
    persist();
    step("mail-rule-saved", {
        id: result.rule.id,
        mailbox: result.rule.mailbox,
        enabled: result.rule.enabled,
        replacing: at === -1 ? null : result.rule.id,
    });
    return result;
}

/** Remove one rule. False when there was none with that id. */
export function remove(id: string): boolean {
    ensureLoaded();
    const at = rules.findIndex((r) => r.id === id);
    if (at === -1) return false;
    const [gone] = rules.splice(at, 1);
    persist();
    step("mail-rule-removed", { id, mailbox: gone?.mailbox ?? "" });
    return true;
}

/** Drop everything in memory. Tests only. */
export function reset(): void {
    rules = [];
    loaded = false;
}
