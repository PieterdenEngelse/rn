/**
 * Writing credentials from the page, and the properties that make it safe.
 *
 * Three things this file exists to hold down, in order of how much it would
 * cost to get them wrong:
 *
 * **A value never comes back.** Not from a describe, not from an error, not
 * from a log line. `docs/token-sec.md` is the standing rule — a panel renders
 * existence, not content — and the test for it is a blunt one: put a
 * distinctive value in, then assert that string appears nowhere in anything
 * this module hands out.
 *
 * **The file's format cannot be injected into.** It is line-oriented and the
 * launcher parses it by splitting on `=`, so a newline in a value would write a
 * second credential nobody typed.
 *
 * **A hand-written file survives being edited.** Comments, ordering and
 * unrelated keys are somebody's work; a save that flattened them would be a
 * small betrayal every time.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync, rmSync, statSync, mkdirSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const DIR = join(tmpdir(), `rn-creds-test-${process.pid}`);
const FILE = join(DIR, "credentials");
(config as unknown as { credentialsPath: string }).credentialsPath = FILE;

const creds = await import("../src/credentials-file.ts");
const secrets = await import("../src/secrets.ts");

/** Distinctive enough that finding it anywhere is unambiguous. */
const VALUE = "ghp_thisIsTheSecretValueAndMustNeverBeRenderedAnywhere";
const NAME = "githubToken";
const VAR = "RN_SECRET_GITHUB_TOKEN";

beforeEach(() => {
    rmSync(DIR, { recursive: true, force: true });
    mkdirSync(DIR, { recursive: true });
    delete process.env[VAR];
});

after(() => {
    rmSync(DIR, { recursive: true, force: true });
    delete process.env[VAR];
});

// --- the rule --------------------------------------------------------------

test("nothing this module returns carries the value", () => {
    assert.deepEqual(creds.set(NAME, VALUE).errors, []);

    // Everything the API can hand a page, serialised and searched. A masked
    // form, a prefix or a length would all fail this, which is the point:
    // "starts with ghp_" confirms a guess and a length narrows a search.
    const surfaces = [
        JSON.stringify(creds.describe(NAME, ["some-job"])),
        JSON.stringify(creds.describeVariable(VAR)),
        JSON.stringify([...creds.fileVars()]),
        JSON.stringify(creds.fileState()),
    ];
    for (const surface of surfaces) {
        assert.equal(surface.includes(VALUE), false, surface);
        // Not even the first few characters.
        assert.equal(surface.includes(VALUE.slice(0, 8)), false, surface);
    }
});

test("a refusal does not quote the value back", () => {
    // An error is written to a log and rendered on a page — the two places the
    // whole rule exists to keep a credential out of. So "expected X, got Y" is
    // exactly the tempting mistake, and it is tested against.
    const errors = creds.set(NAME, `${VALUE}\nRN_SECRET_SNUCK_IN=x`).errors;
    assert.equal(errors.length > 0, true);
    assert.equal(errors.join(" ").includes(VALUE.slice(0, 8)), false, errors.join(" "));
});

// --- what may be written ---------------------------------------------------

test("a value with a line break is refused, because the file is line-oriented", () => {
    for (const bad of [`${VALUE}\nRN_SECRET_EVIL=x`, `${VALUE}\rmore`, "a\nb"]) {
        assert.equal(creds.set(NAME, bad).errors.length > 0, true);
    }
    // Nothing was written, and nothing was applied.
    assert.equal(process.env[VAR], undefined);
    assert.throws(() => readFileSync(FILE, "utf8"));
});

test("an empty value is a removal, and is refused as a save", () => {
    assert.match(creds.set(NAME, "").errors.join(" "), /empty/);
});

test("a name that is not a credential name is refused", () => {
    for (const bad of ["", "9leading", "has space", "has-dash"]) {
        assert.equal(creds.set(bad, VALUE).errors.length > 0, true, bad);
    }
});

test("an oversized value is refused, and only its cap is named", () => {
    const errors = creds.set(NAME, "x".repeat(creds.MAX_VALUE_BYTES + 1)).errors;
    assert.match(errors.join(" "), /longer than/);
});

// --- what a save does ------------------------------------------------------

test("a save applies to this process and to the file, in that order", () => {
    creds.set(NAME, VALUE);
    // The environment is what arms redaction, so it must be live the moment the
    // value exists anywhere — a credential in the file but not the environment
    // is one rn does not yet know to scrub out of a run record.
    assert.equal(secrets.isSet(NAME), true);
    assert.equal(readFileSync(FILE, "utf8").includes(`${VAR}=${VALUE}`), true);
    assert.equal(creds.describe(NAME).set, true);
    assert.equal(creds.describe(NAME).inFile, true);
});

test("redaction covers a credential the moment it is saved", () => {
    creds.set(NAME, VALUE);
    // The property the ordering exists for: a run finishing one millisecond
    // after the save must not be able to write the value into its record.
    assert.equal(secrets.redact(`token is ${VALUE} here`), "token is [redacted] here");
});

test("the file is created 0600, and an existing loose mode is corrected", () => {
    creds.set(NAME, VALUE);
    assert.equal(statSync(FILE).mode & 0o777, 0o600);

    // The launcher only warns about a world-readable file, correctly — refusing
    // to boot over a permission bit leaves someone with no UI in which to fix
    // it. This is the other half: the write path asserts the mode.
    chmodSync(FILE, 0o644);
    assert.equal(creds.fileState().permissionWarning !== undefined, true);
    creds.set("otherToken", "second-value-entirely");
    assert.equal(statSync(FILE).mode & 0o777, 0o600);
    delete process.env.RN_SECRET_OTHER_TOKEN;
});

test("comments, ordering and unrelated keys survive a save", () => {
    writeFileSync(
        FILE,
        [
            "# a header somebody wrote",
            "",
            "RN_SECRET_KEPT=untouched",
            "# a comment about the next one",
            `${VAR}=old-value`,
            "",
        ].join("\n"),
        { mode: 0o600 },
    );

    creds.set(NAME, VALUE);
    const after = readFileSync(FILE, "utf8");
    assert.match(after, /# a header somebody wrote/);
    assert.match(after, /RN_SECRET_KEPT=untouched/);
    assert.match(after, /# a comment about the next one/);
    // Replaced in place rather than appended, so the comment above it still
    // describes the line below it.
    assert.equal(after.includes("old-value"), false);
    assert.equal(after.indexOf("# a comment about the next one") < after.indexOf(VAR), true);
    // And exactly one line for it, not two.
    assert.equal(after.split("\n").filter((l) => l.startsWith(`${VAR}=`)).length, 1);
});

test("a second credential is appended without disturbing the first", () => {
    creds.set(NAME, VALUE);
    creds.set("slackWebhook", "https://hooks.slack.com/services/whatever");
    const after = readFileSync(FILE, "utf8");
    assert.equal(after.split("\n").filter((l) => l.startsWith("RN_SECRET_")).length, 2);
    assert.equal(creds.describe(NAME).set, true);
    assert.equal(creds.describe("slackWebhook").set, true);
    delete process.env.RN_SECRET_SLACK_WEBHOOK;
});

// --- removal ---------------------------------------------------------------

test("removing takes it out of the process and the file", () => {
    creds.set(NAME, VALUE);
    assert.equal(creds.clear(NAME), true);
    // Both halves: leaving the environment reports it as still set, which is
    // true and useless; leaving the file brings it back on the next restart.
    assert.equal(secrets.isSet(NAME), false);
    assert.equal(readFileSync(FILE, "utf8").includes(VAR), false);
});

test("removing something that was never there is false, not an error", () => {
    assert.equal(creds.clear("neverSet"), false);
});

// --- what the board reads --------------------------------------------------

test("set and inFile are separate questions, and the gap is reportable", () => {
    // In the file, not in this process — the launcher has not re-read it.
    writeFileSync(FILE, `${VAR}=from-the-file\n`, { mode: 0o600 });
    assert.equal(creds.describe(NAME).inFile, true);
    assert.equal(creds.describe(NAME).set, false);

    // In this process, not in the file — it disappears at the next restart.
    rmSync(FILE, { force: true });
    process.env[VAR] = VALUE;
    assert.equal(creds.describe(NAME).inFile, false);
    assert.equal(creds.describe(NAME).set, true);
});

test("a missing file is an ordinary state, not a failure", () => {
    const state = creds.fileState();
    assert.equal(state.exists, false);
    assert.equal(state.permissionWarning, undefined);
    assert.deepEqual([...creds.fileVars()], []);
});

test("an undeclared variable is described under its own spelling", () => {
    // The name→variable mapping is one-way, so there is no name to recover.
    // Running the variable back through describe() would ask for
    // RN_SECRET_RN_SECRET_GITHUB_TOKEN — a row telling somebody to set a
    // variable nothing reads.
    writeFileSync(FILE, "RN_SECRET_SOMETHING_ODD=x\n", { mode: 0o600 });
    const entry = creds.describeVariable("RN_SECRET_SOMETHING_ODD");
    assert.equal(entry.envVar, "RN_SECRET_SOMETHING_ODD");
    assert.equal(entry.inFile, true);

    // The concrete bug it avoids: a variable fed back through describe() is
    // treated as a credential *name* and prefixed again.
    assert.equal(
        creds.describe("RN_SECRET_SOMETHING_ODD").envVar,
        "RN_SECRET_RN_SECRET_SOMETHING_ODD",
    );
});
