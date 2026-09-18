/**
 * Every store a worktree writes must be a store that worktree owns.
 *
 * `be/d` points each `RN_*_PATH` at `~/.cache/rn-state-<worktree>` so that a
 * backend run out of `~/ca` cannot write the install somebody actually uses.
 * That list is hand-maintained, and nothing checked it: adding the watched-page
 * store on 2026-09-18 added a path to `config.ts` and not a line to `be/d`, so
 * a page added while testing would have been fetched hourly by the real
 * install and reported onto its pages. It was caught by eye, on a page that
 * happened to print its own path.
 *
 * The failure is quiet in the worst way. Nothing errors, nothing is logged, and
 * the only symptom is a file changing under an install nobody was working on —
 * which is exactly the shape of thing a test should be doing instead of a
 * person.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const be = dirname(dirname(fileURLToPath(import.meta.url)));
const configSource = readFileSync(join(be, "src/config.ts"), "utf8");
const devSource = readFileSync(join(be, "d"), "utf8");

/**
 * Paths that are already per-worktree without anybody exporting anything.
 *
 * `RN_PROFILE_DIR` is where V8 writes profiling artifacts, and it defaults to
 * the process's working directory — which for a worktree's backend *is* that
 * worktree. Exporting it would be a second statement of the same fact, and the
 * two could then disagree.
 */
const ALREADY_SCOPED = new Set(["RN_PROFILE_DIR"]);

/** Every `RN_*_PATH` or `RN_*_DIR` that config.ts reads. */
function storePathsInConfig(): string[] {
    const found = new Set<string>();
    for (const m of configSource.matchAll(/process\.env\.(RN_[A-Z0-9_]*(?:PATH|DIR))\b/g)) {
        found.add(m[1]!);
    }
    for (const m of configSource.matchAll(/process\.env\["(RN_[A-Z0-9_]*(?:PATH|DIR))"\]/g)) {
        found.add(m[1]!);
    }
    return [...found].filter((k) => !ALREADY_SCOPED.has(k)).sort();
}

/** Every such variable `be/d` exports for a worktree. */
function storePathsInDevScript(): Set<string> {
    const found = new Set<string>();
    for (const m of devSource.matchAll(/export\s+(RN_[A-Z0-9_]*(?:PATH|DIR))=/g)) {
        found.add(m[1]!);
    }
    return found;
}

test("every store config.ts reads is pointed at the worktree's own state by be/d", () => {
    const exported = storePathsInDevScript();
    const missing = storePathsInConfig().filter((k) => !exported.has(k));
    assert.deepEqual(
        missing,
        [],
        `be/d does not scope ${missing.join(", ")} to the worktree, so a backend run from ` +
            `~/ca or ~/cb writes the real install's file. Add an export beside the others, ` +
            `with a line saying what sharing it would do.`,
    );
});

test("be/d scopes nothing config.ts does not read", () => {
    // The other direction, and the one a typo makes: exporting
    // RN_WATCH_PAGE_PATH for a config.ts that reads RN_WATCH_PAGES_PATH looks
    // exactly like scoping the store and does nothing at all.
    const read = new Set([...storePathsInConfig(), ...ALREADY_SCOPED]);
    const stray = [...storePathsInDevScript()].filter((k) => !read.has(k)).sort();
    assert.deepEqual(
        stray,
        [],
        `be/d exports ${stray.join(", ")}, which config.ts never reads — either it is misspelt ` +
            `or the setting it scoped is gone.`,
    );
});
