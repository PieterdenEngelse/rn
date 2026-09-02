/**
 * Writing the credentials file, and never reading a value back out of it.
 *
 * **The direction is the design.** `docs/token-sec.md` is the governing rule:
 * a panel renders existence, not content. So this module takes a value in and
 * has no function that gives one out — `describe()` answers "is it set", which
 * is `secrets.isSet()`, which is a boolean. Nothing here returns a credential,
 * and nothing here logs one.
 *
 * **Why the backend may write a file the launcher owns.** It is the launcher
 * that reads `~/.config/rn/credentials` and hands the values into the sealed
 * child — see `launcher/src/credentials.rs` — and that stays true. This module
 * does not change how a credential *reaches* the process; it changes where a
 * person can put one, from "open a dotfile in an editor" to "type it into the
 * page that just told you it was missing".
 *
 * **Why this is not a new hole**, spelled out because "the API writes secrets"
 * deserves better than a shrug. The API has no authentication, so the question
 * for any endpoint is what a local process gains — and on a developer machine
 * "local" is every postinstall script and editor extension. This gains it
 * nothing: it runs as the user, so it can already write this file directly with
 * `fs`, and it can already `POST /api/jobs/:id` to run any automation without
 * holding a credential at all. A *read* endpoint would hand it something new.
 * This one does not. The thing that would change the calculus is the API on a
 * routable address, which `remoteBindRefusal` and `RN_ALLOW_REMOTE` already
 * make a deliberate act.
 *
 * ## Two effects per save, in this order
 *
 * A saved credential is applied to `process.env` *first* and written to the
 * file second, and the order is not incidental:
 *
 * - **`process.env` first** because that is what arms redaction.
 *   `secrets.redact()` scrubs every configured value out of everything a job
 *   reports, and it finds those values by scanning `RN_SECRET_*` at call time.
 *   A credential that existed in the file but not the environment would be a
 *   secret rn does not yet know to scrub — so the window where a run could
 *   write it into a record is closed before the value touches the disk.
 * - **the file second** so it survives a restart. The launcher re-reads it on
 *   every respawn, so what is applied now and what comes back later are the
 *   same value rather than two that can drift.
 *
 * The consequence worth telling the user: a credential works immediately, no
 * restart. The page says so, because "saved, now restart" is advice people
 * follow needlessly forever once given.
 */

import { readFileSync, writeFileSync, mkdirSync, statSync, chmodSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "./config.ts";
import { warn, step } from "./log.ts";
import * as secrets from "./secrets.ts";
import type { CredentialEntry } from "./generated/wire.ts";

export type { CredentialEntry } from "./generated/wire.ts";

/** `githubToken` — the spelling `secrets.envVarFor` derives a variable from. */
const NAME_PATTERN = /^[A-Za-z][A-Za-z0-9_]{0,63}$/;

/**
 * How long a credential may be.
 *
 * Generous — a JWT or a PEM-ish blob fits — and bounded because this is written
 * to a line-oriented file and held in an environment block. Anything past it is
 * a paste of the wrong thing.
 */
export const MAX_VALUE_BYTES = 8192;

/**
 * The file's mode, set on every write.
 *
 * Enforced rather than assumed: the launcher only *warns* about a
 * world-readable credentials file, on stderr, at boot — correctly, since
 * refusing to start over a permission bit leaves someone with no UI in which to
 * fix it. This is the other half of that: the one place that creates the file
 * makes it 0600, and re-asserts it on every write in case something else did
 * not.
 */
const MODE = 0o600;

interface FileState {
    /** The lines exactly as they are, comments and all. */
    lines: string[];
    exists: boolean;
    /** Names the file assigns, in `RN_SECRET_X` form. */
    vars: Set<string>;
    permissionWarning?: string;
}

/**
 * Read the file's *structure*. Values are parsed off and dropped on the floor.
 *
 * Deliberately: this function is the only thing in the backend that opens the
 * file, and if it returned values then something else eventually would render
 * one. The lines are kept whole so a write can put them back untouched —
 * comments, blank lines, ordering and all. A rewrite that lost the header
 * comments would be a small betrayal every time somebody edited a credential.
 */
function read(): FileState {
    let text: string;
    try {
        text = readFileSync(config.credentialsPath, "utf8");
    } catch (err) {
        const e = err as NodeJS.ErrnoException;
        if (e.code !== "ENOENT") {
            warn("credentials-file-unreadable", {
                path: config.credentialsPath,
                reason: e.message ?? String(err),
                effect: "the page cannot say which credentials the file holds; the running values are unaffected",
            });
        }
        return { lines: [], exists: false, vars: new Set() };
    }

    const lines = text.split("\n");
    const vars = new Set<string>();
    for (const line of lines) {
        const key = keyOf(line);
        if (key !== undefined) vars.add(key);
    }

    let permissionWarning: string | undefined;
    try {
        const mode = statSync(config.credentialsPath).mode & 0o777;
        if ((mode & 0o077) !== 0) {
            permissionWarning =
                `readable by others (mode ${mode.toString(8)}) — it holds tokens in plaintext`;
        }
    } catch {
        // A file we just read whose mode we cannot stat is not worth a second
        // failure path; the absence of a warning is the safe direction here
        // only because the write path sets the mode regardless.
    }

    return {
        lines,
        exists: true,
        vars,
        ...(permissionWarning === undefined ? {} : { permissionWarning }),
    };
}

/**
 * The `RN_SECRET_*` key a line assigns, or `undefined`.
 *
 * Matches the launcher's parser in `launcher/src/credentials.rs`: trimmed,
 * `#` is a comment, the key is what precedes the first `=`. The value side is
 * never returned — this function's whole job is to answer "which key does this
 * line set" without carrying what it sets it to.
 */
function keyOf(line: string): string | undefined {
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) return undefined;
    const eq = trimmed.indexOf("=");
    if (eq === -1) return undefined;
    const key = trimmed.slice(0, eq).trim();
    return key.startsWith("RN_SECRET_") ? key : undefined;
}

function persist(lines: string[]): void {
    mkdirSync(dirname(config.credentialsPath), { recursive: true });
    writeFileSync(config.credentialsPath, lines.join("\n"), { encoding: "utf8", mode: MODE });
    // Again after the write: `mode` on writeFileSync applies only when the file
    // is created, so an existing 0644 file would keep its bits silently.
    chmodSync(config.credentialsPath, MODE);
}

/** The header a file this module creates starts with. */
const HEADER = [
    "# rn credentials — one RN_SECRET_<NAME>=value per line.",
    "#",
    "# Read by the launcher on every start and passed into the backend",
    "# explicitly. Outside the install tree, which is replaced wholesale on",
    "# upgrade. Plaintext: anyone who can read this home directory can read",
    "# these. See docs/sec.md.",
    "",
];

/**
 * Everything wrong with a name and value, as sentences.
 *
 * **The value is never quoted back in an error**, however tempting "expected X,
 * got Y" is. An error message is written to a log and rendered on a page, which
 * are the two places `docs/token-sec.md` exists to keep a credential out of.
 */
export function validate(name: string, value: string): string[] {
    const errors: string[] = [];
    if (!NAME_PATTERN.test(name)) {
        errors.push(
            "name must be a credential name like githubToken — letters and digits, starting with a letter",
        );
    }
    if (value === "") {
        errors.push("value is empty — to remove a credential, use the remove control instead");
    }
    if (Buffer.byteLength(value, "utf8") > MAX_VALUE_BYTES) {
        // The length is the one fact about a value that may be stated, and only
        // because it is already over a published cap.
        errors.push(`value is longer than ${MAX_VALUE_BYTES} bytes, which is not a credential`);
    }
    if (/[\r\n]/.test(value)) {
        // The whole of the injection surface for a line-oriented file: a
        // newline in a value would write a second line the launcher then reads
        // as another credential.
        errors.push("value contains a line break, which this file's format cannot hold");
    }
    return errors;
}

/**
 * Set one credential: in this process now, and in the file for the next one.
 *
 * Returns what is wrong, or nothing. Never echoes the value, and the `step`
 * line names the credential only — a log is a file and a page.
 */
export function set(name: string, value: string): { errors: string[] } {
    const errors = validate(name, value);
    if (errors.length > 0) return { errors };

    const key = secrets.envVarFor(name);

    // Environment first — see the module doc. This is what arms redaction, and
    // it must be armed before the value is anywhere it could be read back from.
    process.env[key] = value;

    const state = read();
    const line = `${key}=${value}`;
    let replaced = false;
    const lines = state.lines.map((existing) => {
        if (keyOf(existing) !== key) return existing;
        replaced = true;
        return line;
    });

    if (!replaced) {
        if (!state.exists) lines.push(...HEADER);
        // Trailing blank lines are where an appended line belongs, but a file
        // that ends without a newline would otherwise get its last line glued
        // to this one.
        if (lines.length > 0 && lines[lines.length - 1] !== "") lines.push("");
        lines.push(line, "");
    }

    try {
        persist(lines);
    } catch (err) {
        warn("credentials-file-not-written", {
            credential: name,
            path: config.credentialsPath,
            reason: err instanceof Error ? err.message : String(err),
            effect: "the credential works now and is gone after the next restart",
        });
        return {
            errors: [
                "the credential is in use now, but could not be written to the file — it will be gone after a restart",
            ],
        };
    }

    step("credential-set", { credential: name, envVar: key, replaced });
    return { errors: [] };
}

/**
 * Remove one: from this process and from the file.
 *
 * `false` when neither had it. Both halves matter — leaving the environment
 * would report it as still set, which is true and useless, and leaving the file
 * would bring it back on the next restart.
 */
export function clear(name: string): boolean {
    const key = secrets.envVarFor(name);
    const had = process.env[key] !== undefined;
    delete process.env[key];

    const state = read();
    const lines = state.lines.filter((line) => keyOf(line) !== key);
    const removedFromFile = lines.length !== state.lines.length;

    if (removedFromFile) {
        try {
            persist(lines);
        } catch (err) {
            warn("credentials-file-not-written", {
                credential: name,
                path: config.credentialsPath,
                reason: err instanceof Error ? err.message : String(err),
                effect: "the credential is out of use now and returns on the next restart",
            });
        }
    }

    if (had || removedFromFile) step("credential-cleared", { credential: name });
    return had || removedFromFile;
}

/**
 * One row for the page: name, variable, and the two booleans. Never a value.
 *
 * `inFile` and `set` are separate questions and the gap between them is the
 * useful part — set but not in the file is a value that disappears at the next
 * restart, and in the file but not set is a file the launcher has not re-read.
 */
export function describe(name: string, declaredBy: string[] = []): CredentialEntry {
    const state = read();
    return {
        name,
        envVar: secrets.envVarFor(name),
        set: secrets.isSet(name),
        inFile: state.vars.has(secrets.envVarFor(name)),
        declaredBy,
    };
}

/**
 * A row for a variable the file sets that nothing declares.
 *
 * Its own function because the name→variable mapping is one-way on purpose —
 * `envVarFor` turns `githubToken` into `RN_SECRET_GITHUB_TOKEN` and nothing
 * reverses it — so an undeclared variable has no name to show. Running it back
 * through `describe()` would produce `RN_SECRET_RN_SECRET_GITHUB_TOKEN`, which
 * is a row telling somebody to set a variable nothing reads.
 *
 * Shown at all because a credential put in the file ahead of the job that will
 * want it is a reasonable thing to have done, and a board that hid it would
 * look like it had lost the value.
 */
export function describeVariable(variable: string): CredentialEntry {
    return {
        name: variable,
        envVar: variable,
        set: process.env[variable] !== undefined && process.env[variable] !== "",
        inFile: read().vars.has(variable),
        declaredBy: [],
    };
}

/** What the file names, as credential-shaped variables. Names only. */
export function fileVars(): Set<string> {
    return read().vars;
}

/** Whether the file is there, and anything the user should know about it. */
export function fileState(): { exists: boolean; permissionWarning?: string } {
    const state = read();
    return {
        exists: state.exists,
        ...(state.permissionWarning === undefined
            ? {}
            : { permissionWarning: state.permissionWarning }),
    };
}
