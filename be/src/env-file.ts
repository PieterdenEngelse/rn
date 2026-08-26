/**
 * Reading `be/.env` back, to compare it against what the process has.
 *
 * ## Why the vocabulary comes from `.env.example`
 *
 * A value is only shown for a key rn recognises, and "recognises" is defined as
 * "appears in `be/.env.example`". That looks indirect and is the opposite:
 * `be/test/env-example.test.ts` holds the example equal, in both directions, to
 * what `config.ts` actually reads. So the example is a test-enforced list of
 * every setting this app has — and adding one to `config.ts` already requires
 * adding it there, which now also makes it displayable, with no third list to
 * keep in step.
 *
 * The alternative was re-deriving the keys by regex over `config.ts` at
 * runtime, which is what the test does. A test may parse source; a request
 * handler should not.
 *
 * ## Why unknown keys are named but never valued
 *
 * `.env` is a file people put things in. `.env.example` says credentials do not
 * belong there — they live in `~/.config/rn/credentials`, outside the install
 * tree — but nothing enforces that on `.env` itself, and people paste tokens
 * into dotfiles. A key rn does not recognise is a key rn cannot vouch for, so
 * it is reported by name and as set, never by value: the same rule
 * `secrets.describe()` follows for credentials, for the same reason.
 *
 * Known values are still passed through `secrets.redact()` on the way out. That
 * is not redundant — it catches a configured credential that someone also put
 * in `.env` under a name that happens to be recognised.
 */

import { readFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as secrets from "./secrets.ts";
import { display } from "./paths.ts";
import type { EnvEntry, EnvResponse } from "./generated/wire.ts";

/** `be/`, from this module's own location, so a rename cannot desync it. */
const BE_DIR = dirname(dirname(fileURLToPath(import.meta.url)));

export const ENV_PATH = join(BE_DIR, ".env");
const EXAMPLE_PATH = join(BE_DIR, ".env.example");

/**
 * `KEY=value` pairs, ignoring comments and blank lines.
 *
 * Deliberately not a dotenv implementation. It does not do quoting, escapes,
 * multi-line values or interpolation, because it is not what loads the file —
 * the runtime does that, via `--env-file-if-exists`. This only needs to answer
 * "which keys does the file mention, and what does it appear to say", and a
 * parser that diverged from the runtime's in the corners would report a
 * difference that is its own fault as drift.
 */
export function parseEnv(source: string): Map<string, string> {
    const out = new Map<string, string>();
    for (const line of source.split("\n")) {
        const trimmed = line.trim();
        if (trimmed === "" || trimmed.startsWith("#")) continue;
        const eq = trimmed.indexOf("=");
        if (eq <= 0) continue;
        out.set(trimmed.slice(0, eq).trim(), trimmed.slice(eq + 1).trim());
    }
    return out;
}

/** Every key the example documents, set or commented out — both count. */
export function knownKeys(source: string): Set<string> {
    const found = new Set<string>();
    for (const line of source.split("\n")) {
        const m = /^#?\s*([A-Z][A-Z0-9_]*)=/.exec(line.trim());
        if (m) found.add(m[1]!);
    }
    return found;
}

/**
 * Pure so it can be tested without a filesystem, which is what makes the
 * unknown-key rule checkable rather than merely commented.
 */
export function reconcile(
    file: Map<string, string>,
    processEnv: Record<string, string | undefined>,
    known: Set<string>,
): EnvEntry[] {
    // The union, so a key set in the real environment but absent from the file
    // still appears — that is the precedence rule made visible, and it is one
    // of the two things this board exists to show.
    const keys = [...new Set([...file.keys(), ...known])].sort();

    const entries: EnvEntry[] = [];
    for (const key of keys) {
        const inFile = file.has(key);
        const live = processEnv[key];
        // A key that is neither in the file nor in the process is one the
        // example merely documents. Listing it as a row of blanks would bury
        // the rows that say something.
        if (!inFile && live === undefined) continue;

        const isKnown = known.has(key);
        const fileValue = inFile ? file.get(key)! : undefined;
        entries.push({
            key,
            // The value only for a key we recognise. `inFile` still carries
            // the fact that it is set, which is what a reader needs.
            ...(isKnown && fileValue !== undefined
                ? { fileValue: secrets.redact(fileValue) }
                : {}),
            inFile,
            ...(isKnown && live !== undefined ? { processValue: secrets.redact(live) } : {}),
            known: isKnown,
            // Only meaningful for a key we can see both sides of. An unknown
            // key is reported as never drifting rather than as always
            // drifting, because "we cannot tell" must not read as a warning.
            drifted: isKnown && inFile && fileValue !== live,
        });
    }
    return entries;
}

export function describeEnv(): EnvResponse {
    const exists = existsSync(ENV_PATH);
    const file = exists ? parseEnv(readFileSync(ENV_PATH, "utf8")) : new Map<string, string>();
    const known = existsSync(EXAMPLE_PATH)
        ? knownKeys(readFileSync(EXAMPLE_PATH, "utf8"))
        : new Set<string>();

    const entries = reconcile(file, process.env, known);
    return {
        path: display(ENV_PATH),
        exists,
        entries,
        drifted: entries.filter((e) => e.drifted).length,
    };
}
