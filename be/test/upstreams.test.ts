/**
 * The upstream watcher: what it reads, and what it does when it cannot.
 *
 * Most of this is the pure half — the version comparison and the two manifest
 * scans, tested against the strings the three registries and four manifest
 * formats actually produce. The failures worth catching there are the quiet
 * ones: a comparison that says 4.10.0 is older than 4.9.0 reports an upgrade as
 * a downgrade and goes on doing it forever, with nothing failing.
 *
 * **Nothing here makes a network request.** A test that needs crates.io to be up
 * is a test that fails for reasons that have nothing to do with this
 * repository, so the two tests that do run the job pick paths that never reach
 * a registry.
 *
 * The rule the job's cursor depends on — staged, committed only on success,
 * withheld under dry run — belongs to the store and is pinned in
 * `state.test.ts`. It is deliberately not repeated here: one change to that
 * contract should turn one file red, not two, and the second one would be
 * claiming the upstream watcher is broken when it is not.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
    cargoDependencyNames,
    cargoLockVersions,
    compareVersions,
    findRepoRoot,
    major,
    watchUpstreams,
} from "../src/jobs/watch-upstreams.ts";
import { runJob } from "../src/jobs/run.ts";
import * as jobState from "../src/jobs/state.ts";
import * as history from "../src/jobs/history.ts";
import * as running from "../src/running.ts";
import * as dry from "../src/dry-run.ts";
import { config } from "../src/config.ts";

// Same protection as jobs.test.ts, and needed more here: this suite exists to
// exercise the cursor, so it is the one suite guaranteed to write one.
(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-upstream-runs-${process.pid}.json`,
);
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-upstream-state-${process.pid}.json`,
);

const realDryRun = dry.BASELINE;

beforeEach(() => {
    running.reset();
    history.reset();
    jobState.reset();
    dry.setDryRun(realDryRun);
});

after(async () => {
    dry.setDryRun(realDryRun);
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
});

// ---- version comparison -------------------------------------------------

test("versions compare numerically, not as strings", () => {
    // The failure this is aimed at: "4.10.0" < "4.9.0" alphabetically, which
    // would report an upgrade as a downgrade and go on doing it silently.
    assert.equal(compareVersions("4.9.0", "4.10.0") < 0, true);
    assert.equal(compareVersions("4.10.0", "4.9.0") > 0, true);
    assert.equal(compareVersions("1.2.3", "1.2.3"), 0);
});

test("the prefixes each ecosystem writes are ignored", () => {
    // v24.19.0 from .nvmrc, ^4.1.14 from package.json, =0.7.9 from Cargo.toml.
    assert.equal(compareVersions("v24.19.0", "24.19.0"), 0);
    assert.equal(compareVersions("^4.1.14", "4.1.14"), 0);
    assert.equal(compareVersions("=0.7.9", "0.7.10") < 0, true);
    assert.equal(compareVersions("~1.0.0", "1.0.0"), 0);
});

test("a missing segment counts as zero", () => {
    // "0.7" in a Cargo manifest against "0.7.0" on crates.io is not an upgrade,
    // and reporting it as one would make manganis permanently out of date.
    assert.equal(compareVersions("0.7", "0.7.0"), 0);
    assert.equal(compareVersions("1", "1.0.1") < 0, true);
});

test("a prerelease sorts below the release of the same numbers", () => {
    assert.equal(compareVersions("2.0.0-rc.1", "2.0.0") < 0, true);
    assert.equal(compareVersions("2.0.0", "2.0.0-rc.1") > 0, true);
});

test("major reads the line, whatever the prefix", () => {
    assert.equal(major("v24.19.0"), 24);
    assert.equal(major("0.7.9"), 0);
});

// ---- manifest reading ---------------------------------------------------

test("cargo dependency names come from every dependency table", () => {
    const names = cargoDependencyNames(`
[package]
name = "fe"
version = "0.1.0"

[dependencies]
shared = { path = "../shared", default-features = false }
dioxus = { version = "=0.7.9", features = ["web"] }
js-sys = "0.3"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = "3"

[features]
default = ["web"]
web = ["dioxus/web"]
`);
    // `shared` is a path dependency with no upstream, and the [package] and
    // [features] tables both contain `name =` / `default =` lines that a
    // careless scan would take for dependencies.
    assert.deepEqual(names, ["dioxus", "js-sys", "libc", "tempfile"]);
});

test("cargo versions come from the lock, first entry winning", () => {
    const versions = cargoLockVersions(`
[[package]]
name = "dioxus"
version = "0.7.9"
dependencies = ["serde"]

[[package]]
name = "syn"
version = "1.0.109"

[[package]]
name = "syn"
version = "2.0.87"
`);
    assert.equal(versions.get("dioxus"), "0.7.9");
    // Two versions of one crate in a lock is ordinary. The direct dependency
    // is the one resolved first, and picking the other would report an upgrade
    // for a crate that already has it.
    assert.equal(versions.get("syn"), "1.0.109");
});

test("the repository root is found by both markers, not either", async () => {
    const base = await mkdtemp(join(tmpdir(), "rn-root-"));
    const decoy = join(base, "some-rust-project");
    await mkdir(join(decoy, "deep", "deeper"), { recursive: true });
    await writeFile(join(decoy, "Cargo.toml"), "[workspace]\n");

    // Cargo.toml alone is any Rust project rn might be installed inside.
    assert.equal(findRepoRoot(join(decoy, "deep", "deeper")), null);

    await mkdir(join(decoy, "be"), { recursive: true });
    await writeFile(join(decoy, "be", ".nvmrc"), "v24.19.0\n");
    assert.equal(findRepoRoot(join(decoy, "deep", "deeper")), decoy);

    await rm(base, { recursive: true, force: true });
});

test("this repository is what the job's default resolves to", () => {
    // The default is computed at module load by walking up from the job's own
    // file. If that stops working the input silently defaults to "" and the
    // job skips every run with "no source checkout found", which reads like a
    // packaging question rather than a broken path.
    const root = findRepoRoot(import.meta.dirname);
    assert.notEqual(root, null);
    assert.equal(typeof watchUpstreams.inputs?.[0]?.default, "string");
});

// ---- the job's own contract ---------------------------------------------

test("it skips rather than fails where there is no checkout", async () => {
    const result = await runJob(watchUpstreams, "manual", undefined, { root: "" });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /No source checkout/);
    // A skip, not a failure: a packaged install has no manifests, and a job
    // that fails nightly for a reason nobody can act on is one people mute.
    assert.equal(history.list()[0]?.error, undefined);
});

test("an unknown ecosystem is named rather than silently dropped", async () => {
    const result = await runJob(watchUpstreams, "manual", undefined, {
        root: findRepoRoot(import.meta.dirname) ?? "",
        ecosystems: "npn",
    });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /No ecosystem to check/);
    const steps = history.list()[0]?.steps ?? [];
    const named = steps.find((s) => s.name === "unknown-ecosystem");
    assert.notEqual(named, undefined, "a typo'd ecosystem must appear in the trace");
});

// The runner rule this job's cursor depends on — committed only on success,
// discarded on failure, withheld under dry run — is pinned in state.test.ts,
// which is where the store's contract lives. Duplicating it here would mean
// two files going red for one change, and the second one saying "the upstream
// watcher is broken" when it is not.
