/**
 * be/.env.example is the reference for what may go in be/.env, and this keeps
 * it true.
 *
 * The file has always said "every key added to .env must be added here too".
 * Nothing enforced it, and it drifted: ten of the twelve variables config.ts
 * reads were missing, so the only way to discover RN_PROFILE_MAX_AGE_DAYS or
 * RN_CORS_ORIGIN existed was to read the source. A reference nobody can rely
 * on is worse than none, because it looks complete.
 *
 * config.ts is the source of truth here for the same reason its own docstring
 * gives — "all defaults and environment reading live here, in one place, so
 * that 'where do I change this?' always has the same answer".
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const be = dirname(dirname(fileURLToPath(import.meta.url)));
const configSource = readFileSync(join(be, "src/config.ts"), "utf8");
const exampleSource = readFileSync(join(be, ".env.example"), "utf8");

/**
 * Variables config.ts reads but nobody sets in .env.
 *
 * HOME and USERPROFILE are read as a fallback for the state directory. They
 * are the operating system's, and listing them would invite someone to
 * override their own home directory in a config file for an automation tool.
 */
const NOT_USER_SETTABLE = new Set(["HOME", "USERPROFILE"]);

/** Every environment variable config.ts reads, in either syntax. */
function envKeysInConfig(): string[] {
    const found = new Set<string>();
    for (const m of configSource.matchAll(/process\.env\.([A-Z][A-Z0-9_]*)/g)) {
        found.add(m[1]!);
    }
    for (const m of configSource.matchAll(/process\.env\["([A-Z][A-Z0-9_]*)"\]/g)) {
        found.add(m[1]!);
    }
    return [...found].filter((k) => !NOT_USER_SETTABLE.has(k)).sort();
}

/** Keys the example documents, set or commented out — both count as listed. */
function keysInExample(): Set<string> {
    const found = new Set<string>();
    for (const line of exampleSource.split("\n")) {
        const m = /^#?\s*([A-Z][A-Z0-9_]*)=/.exec(line.trim());
        if (m) found.add(m[1]!);
    }
    return found;
}

test("every setting config.ts reads is documented in .env.example", () => {
    const documented = keysInExample();
    const missing = envKeysInConfig().filter((k) => !documented.has(k));
    assert.deepEqual(
        missing, [],
        `add these to be/.env.example with a safe placeholder: ${missing.join(", ")}`,
    );
});

test(".env.example documents nothing config.ts does not read", () => {
    // The other direction, so a setting that is removed from the code does not
    // linger here as an instruction that quietly does nothing.
    const read = new Set(envKeysInConfig());
    const stale = [...keysInExample()].filter((k) => !read.has(k)).sort();
    assert.deepEqual(
        stale, [],
        `remove these from be/.env.example, or config.ts stopped reading them: ${stale.join(", ")}`,
    );
});

/**
 * Lines that grant a permission, and must therefore never be live in the
 * example — a copied .env has to be safe on the first run.
 *
 * A table rather than one check, because the file has grown a second such
 * switch and will grow a third. Each is a value that lifts a refusal, not
 * merely a setting with an inconvenient value: getting DRY_RUN wrong is
 * irreversible, and getting RN_ALLOW_REMOTE wrong exposes an API with no
 * authentication on it.
 */
const PERMISSIONS: readonly { key: string; granting: RegExp; cost: string }[] = [
    { key: "DRY_RUN", granting: /^false$/, cost: "would ship an armed copy" },
    {
        key: "RN_ALLOW_REMOTE",
        granting: /^1$/,
        cost: "would pre-authorise a bind the refusal exists to stop",
    },
];

test("the example arms nothing by accident", () => {
    // Commented lines are documentation and are fine — that is how the file
    // shows what a value would look like. A *live* one is an instruction.
    const live = new Map<string, string>();
    for (const line of exampleSource.split("\n")) {
        const m = /^([A-Z][A-Z0-9_]*)=(.*)$/.exec(line.trim());
        if (m) live.set(m[1]!, m[2]!.trim());
    }

    for (const { key, granting, cost } of PERMISSIONS) {
        const value = live.get(key);
        if (value === undefined) continue;
        assert.equal(
            granting.test(value), false,
            `${key}=${value} in the example ${cost}`,
        );
    }
});

test("no credential is documented here", () => {
    // They belong in ~/.config/rn/credentials, outside the install tree —
    // this directory is replaced wholesale on upgrade. See docs/sec.md.
    const secrets = [...keysInExample()].filter((k) => k.startsWith("RN_SECRET_"));
    assert.deepEqual(secrets, [], "credentials do not live in .env — see docs/sec.md");
});
