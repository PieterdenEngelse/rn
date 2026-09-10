/**
 * The last connection test, kept so a page can say when each half last worked.
 *
 * `POST /api/mail-test` proves the account, the host, the port and TLS, and
 * until now that proof lived only in the reply carrying it: a reload erased
 * it. That is worst for the half it matters most to. Reading has watched
 * mailboxes, a schedule and a run history, so "is IMAP fine" has several
 * answers; sending has none at all until a message has gone out, and on this
 * install nothing had ever been sent.
 *
 * ## Why a file rather than a variable
 *
 * A restart is not evidence that anything changed about the mail servers, and
 * an in-memory record would reset on every `be/r` — which on this machine is
 * several times an hour while something is being built. "Last proved" is only
 * worth showing if it outlives the process that proved it.
 *
 * ## What it deliberately does not hold
 *
 * No history, and no address. One record per protocol, replaced each time,
 * because the question is "does it work now" and the answer is the most recent
 * attempt. A list of every test ever run would be a log of when somebody
 * pressed a button, which nobody needs, and it would grow without bound in a
 * file nothing prunes.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { warn } from "../log.ts";
import type { MailTestRecord } from "../generated/wire.ts";

/** Both halves, each absent until it has been tried once. */
export interface Remembered {
    imap?: MailTestRecord;
    smtp?: MailTestRecord;
}

let cache: Remembered | undefined;

function read(): Remembered {
    if (cache !== undefined) return cache;
    try {
        const parsed: unknown = JSON.parse(readFileSync(config.mailTestPath, "utf8"));
        cache = typeof parsed === "object" && parsed !== null ? (parsed as Remembered) : {};
    } catch (err) {
        // Missing is the ordinary state — nobody has pressed the button yet —
        // and says nothing. A corrupt file is worth a line, because the page
        // will then say "never tested" about a connection that was.
        if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
            warn("mail-test-record-unreadable", {
                path: config.mailTestPath,
                reason: String(err),
                effect: "the page reports both halves as never tested",
            });
        }
        cache = {};
    }
    return cache;
}

/** What was last proved, for the boards. */
export function last(): Remembered {
    return { ...read() };
}

/** Replace one half's record. The other is left as it was. */
export function record(half: "imap" | "smtp", result: MailTestRecord): void {
    const next: Remembered = { ...read(), [half]: result };
    cache = next;
    try {
        mkdirSync(dirname(config.mailTestPath), { recursive: true });
        writeFileSync(config.mailTestPath, `${JSON.stringify(next, null, 4)}\n`, {
            mode: 0o600,
        });
    } catch (err) {
        // A record that cannot be persisted is still a record this process can
        // show. Failing the test over it would report a working connection as
        // broken, which is the opposite of what the button is for.
        warn("mail-test-record-unwritable", {
            path: config.mailTestPath,
            reason: String(err),
            effect: "the result is shown now and forgotten at restart",
        });
    }
}

/** Forget what was read, so the next call re-reads. Tests use it. */
export function reset(): void {
    cache = undefined;
}
