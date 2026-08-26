import { test } from "node:test";
import assert from "node:assert/strict";
import { parseEnv, knownKeys, reconcile } from "../src/env-file.ts";

const KNOWN = new Set(["DRY_RUN", "LOG_LEVEL", "BACKEND_PORT"]);

test("a key rn does not recognise is named but never valued", () => {
    // The rule this whole module is shaped around. .env.example says
    // credentials do not belong in .env, but nothing enforces that on .env
    // itself, and people paste tokens into dotfiles. A key rn cannot vouch for
    // gets its name across the boundary and nothing else.
    const file = new Map([["SOME_TOKEN", "ghp_averyrealsecret"]]);

    const [entry] = reconcile(file, {}, KNOWN);

    assert.equal(entry!.key, "SOME_TOKEN");
    assert.equal(entry!.known, false);
    assert.equal(entry!.inFile, true, "the reader still learns it is set");
    assert.equal(entry!.fileValue, undefined, "and never what it is set to");
    assert.equal(entry!.processValue, undefined);
    // Belt and braces: the value must not appear anywhere in the payload.
    assert.ok(!JSON.stringify(entry).includes("ghp_"), "no trace of the value");
});

test("an unknown key never reads as drifted", () => {
    // We cannot compare what we will not look at, and "cannot tell" must not
    // render as a warning — a board that cries drift over every unrecognised
    // key is one people stop reading.
    const file = new Map([["SOME_TOKEN", "a"]]);
    const [entry] = reconcile(file, { SOME_TOKEN: "b" }, KNOWN);
    assert.equal(entry!.drifted, false);
});

test("a known key edited since startup is reported as drifted", () => {
    // The thing the board exists for: the file was changed and the backend has
    // not been restarted, so the two disagree with nothing else saying so.
    const file = new Map([["LOG_LEVEL", "debug"]]);
    const [entry] = reconcile(file, { LOG_LEVEL: "info" }, KNOWN);

    assert.equal(entry!.drifted, true);
    assert.equal(entry!.fileValue, "debug", "what the file says now");
    assert.equal(entry!.processValue, "info", "what the process started with");
});

test("agreement is not drift", () => {
    const file = new Map([["LOG_LEVEL", "info"]]);
    const [entry] = reconcile(file, { LOG_LEVEL: "info" }, KNOWN);
    assert.equal(entry!.drifted, false);
});

test("a variable set in the real environment but not the file still appears", () => {
    // The documented precedence — real environment wins over .env — made
    // visible. Without this row, "the file says nothing and the process has a
    // value" would be invisible, which is the confusing half of that rule.
    const entries = reconcile(new Map(), { BACKEND_PORT: "3999" }, KNOWN);
    const port = entries.find((e) => e.key === "BACKEND_PORT");

    assert.ok(port, "listed");
    assert.equal(port!.inFile, false);
    assert.equal(port!.processValue, "3999");
    assert.equal(port!.drifted, false, "not drift — the file never claimed otherwise");
});

test("a key the example documents but nobody sets is left out", () => {
    // Otherwise every install shows a dozen rows of blanks, and the rows that
    // say something are buried among them.
    assert.deepEqual(reconcile(new Map(), {}, KNOWN), []);
});

test("parseEnv ignores comments and blanks, and keeps the whole value", () => {
    const parsed = parseEnv(
        ["# a comment", "", "DRY_RUN=true", "  LOG_LEVEL = info  ", "#DISABLED=1", "NOEQUALS"].join(
            "\n",
        ),
    );
    assert.equal(parsed.get("DRY_RUN"), "true");
    assert.equal(parsed.get("LOG_LEVEL"), "info", "trimmed on both sides");
    assert.equal(parsed.has("DISABLED"), false, "a commented line is not set");
    assert.equal(parsed.has("NOEQUALS"), false);
});

test("parseEnv keeps a value containing an equals sign", () => {
    // RN_CORS_ORIGIN and a connection string both contain them. Splitting on
    // the last '=' rather than the first would truncate the value.
    const parsed = parseEnv("RN_CORS_ORIGIN=http://a:1?x=1,http://b:2");
    assert.equal(parsed.get("RN_CORS_ORIGIN"), "http://a:1?x=1,http://b:2");
});

test("knownKeys counts commented lines, as the example writes them", () => {
    // Nearly every key in .env.example is commented out, because every one of
    // them is already the default. If commented lines did not count, almost
    // nothing would be recognised.
    const known = knownKeys(["#BACKEND_PORT=3010", "DRY_RUN=true", "# prose"].join("\n"));
    assert.deepEqual([...known].sort(), ["BACKEND_PORT", "DRY_RUN"]);
});
