/**
 * Credentials, referenced by name and never by value.
 *
 * Two halves, and the second is the one that matters most. **Reference by
 * name**: a job declares which credentials it needs and asks for them through
 * `ctx.secret("githubToken")`, so no token is ever written into a job file, a
 * request body, or a run record. **Redaction**: everything a job reports —
 * step details, the summary, the input it was given, an error message — is
 * scrubbed of every configured secret value before it is logged or written to
 * disk.
 *
 * Redaction is not a nicety layered on top. `docs/n8n.md` §6 puts it before the
 * store on purpose: a run record is written to `~/.config/rn/job-runs.json` and
 * rendered on a page, so a job that logs its own token has published it, and
 * the strongest possible store does not undo that. A store in Node with correct
 * redaction beats a store in Rust without it.
 *
 * **Where the values come from: the environment**, one variable per credential,
 * named `RN_SECRET_<NAME>`. The launcher reads them from
 * `~/.config/rn/credentials` and passes them into the sealed child explicitly —
 * outside the install tree, because that tree is replaced wholesale on upgrade
 * and tokens in `be/.env` would have gone with it, silently. See
 * `launcher/src/credentials.rs` and `docs/sec.md`.
 *
 * The file is plaintext, and `docs/sec.md` says so rather than dressing it up.
 * An encrypted file with a key from the OS keychain is the conventional next
 * step and slots in behind `read()` without any job changing, which is the
 * reason this indirection exists at all rather than jobs reading `process.env`.
 *
 * Nothing in this module logs a value, and nothing sends one over the API. What
 * crosses the boundary is a name and whether it is set — see `describe()`.
 */

/**
 * `githubToken` → `RN_SECRET_GITHUB_TOKEN`.
 *
 * A derived name rather than one each job states for itself: two ways to spell
 * the same credential is how a job ends up reading a variable nobody set, and
 * failing at 03:00 with an empty header rather than a missing one.
 */
export function envVarFor(name: string): string {
    const upper = name
        .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
        .replace(/[^A-Za-z0-9]+/g, "_")
        .toUpperCase();
    return `RN_SECRET_${upper}`;
}

/** The value, or undefined when nothing is configured. Never logged. */
export function read(name: string): string | undefined {
    const raw = process.env[envVarFor(name)];
    // An empty variable is not a credential. Treating "" as set is how a
    // request goes out with an empty Authorization header and comes back 401
    // with nothing in the record to explain it.
    return raw === undefined || raw === "" ? undefined : raw;
}

/** Is it configured? The only fact about a credential that leaves the backend. */
export function isSet(name: string): boolean {
    return read(name) !== undefined;
}

/**
 * What a page may know: the name, the variable to set, and whether it is set.
 *
 * Never the value, and never a prefix or a length of it — "starts with ghp_" is
 * enough to confirm a guess, and a length narrows a search.
 */
export function describe(name: string): { name: string; envVar: string; set: boolean } {
    return { name, envVar: envVarFor(name), set: isSet(name) };
}

/**
 * Values short enough to be ordinary text are not redacted.
 *
 * Scrubbing a two-character secret would replace those characters everywhere
 * they appear — inside paths, counts and words — and produce a record that is
 * unreadable and *looks* corrupted rather than protected. Anything a real
 * credential could be is far longer than this; anything shorter is not a
 * credential worth having.
 */
export const MIN_REDACTABLE = 8;

/** What replaces a secret wherever one is found. */
export const REDACTED = "[redacted]";

/**
 * Every configured secret value, longest first.
 *
 * Longest first because one credential can contain another — a URL secret that
 * embeds a token, say — and replacing the short one first would leave the tail
 * of the long one sitting in the record.
 */
function configuredValues(): string[] {
    const values: string[] = [];
    for (const [key, value] of Object.entries(process.env)) {
        if (!key.startsWith("RN_SECRET_")) continue;
        if (value !== undefined && value.length >= MIN_REDACTABLE) values.push(value);
    }
    return values.sort((a, b) => b.length - a.length);
}

/** Replace every configured secret found anywhere in `text`. */
export function redact(text: string): string {
    let out = text;
    for (const value of configuredValues()) {
        // split/join rather than a RegExp: a credential can contain any
        // character, and building a pattern out of one is a way to either
        // match nothing or throw.
        if (out.includes(value)) out = out.split(value).join(REDACTED);
    }
    return out;
}

/**
 * Redact every string inside a value, however deeply nested.
 *
 * Keys as well as values: a job that reports `{ [token]: 1 }` has published it
 * just as surely as one that reports `{ token: value }`. Cycles are not handled
 * because nothing here can produce one — every caller passes something that has
 * already been through, or is about to go through, JSON.
 */
export function scrub<T>(value: T): T {
    if (typeof value === "string") return redact(value) as T;
    if (Array.isArray(value)) return value.map(scrub) as T;
    if (value !== null && typeof value === "object") {
        const out: Record<string, unknown> = {};
        for (const [key, v] of Object.entries(value)) out[redact(key)] = scrub(v);
        return out as T;
    }
    return value;
}
