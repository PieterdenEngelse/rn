/**
 * The permission hint: when it fires, and what it tells you to do.
 *
 * This was two identical functions, one per job, deliberately kept apart until
 * the refusal path had been watched rather than inferred. It is one now, so its
 * contract is pinned once here rather than through whichever job happens to
 * exercise it.
 *
 * The wording in the first test is not invented. It is what deno 2.9.5 produced
 * against a scratch backend granted only rn's own two ports.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { netPermissionHint } from "../src/jobs/net-permission.ts";

const HOSTS = ["nodejs.org", "registry.npmjs.org", "crates.io"];

test("the arm that was actually observed fires", () => {
    const hint = netPermissionHint(
        new Error('Requires net access to "nodejs.org:443", run again with the --allow-net flag'),
        HOSTS,
    );
    assert.match(String(hint), /network allowlist/);
    assert.match(String(hint), /nodejs\.org, registry\.npmjs\.org and crates\.io/);
});

test("the two arms that were not observed stay anyway", () => {
    // An arm that never matches costs nothing. A hint that quietly stops firing
    // leaves a bare permission error on the record and takes the allowlist with
    // it — so these stay until Deno's wording is known to have settled.
    assert.notEqual(netPermissionHint(new Error("PermissionDenied: net access"), HOSTS), undefined);
    assert.notEqual(netPermissionHint(new Error("NotCapable: net"), HOSTS), undefined);
    // The name counts as well as the message: Deno throws Deno.errors.NotCapable
    // with a message that need not repeat the class.
    const named = new Error("net access is required");
    named.name = "NotCapable";
    assert.notEqual(netPermissionHint(named, HOSTS), undefined);
});

test("an ordinary network failure is not dressed up as a permission problem", () => {
    // The failure a too-broad regex would produce: every 503 and every DNS blip
    // telling the user to edit an allowlist that was never the problem.
    for (const message of [
        "fetch failed",
        "503 Service Unavailable from https://crates.io/api/v1/crates/serde",
        "getaddrinfo ENOTFOUND registry.npmjs.org",
        "The operation was aborted",
    ]) {
        assert.equal(netPermissionHint(new Error(message), HOSTS), undefined, message);
    }
});

test("something thrown that is not an Error is still read", () => {
    // A job cannot promise what a runtime throws, and a permission refusal that
    // arrived as a string would otherwise be the one case with no advice.
    assert.notEqual(
        netPermissionHint('Requires net access to "a.example:443"', ["a.example"]),
        undefined,
    );
});

test("the hosts named are the ones the caller passes", () => {
    // The reason this takes an argument at all. The version it grew out of
    // hardcoded three, so a run narrowed to npm was told to allowlist two hosts
    // it never touched.
    const one = netPermissionHint(new Error("Requires net access"), ["registry.npmjs.org"]);
    assert.match(String(one), /add registry\.npmjs\.org to the network allowlist/);
    assert.doesNotMatch(String(one), /crates\.io/);

    const two = netPermissionHint(new Error("Requires net access"), ["a.example", "b.example"]);
    assert.match(String(two), /add a\.example and b\.example to/);
});

test("a duplicated or empty host list still reads as a sentence", () => {
    // Two feeds on one host is ordinary, and the advice is to allowlist it once.
    const dupes = netPermissionHint(new Error("Requires net access"), ["a.example", "a.example"]);
    assert.match(String(dupes), /add a\.example to the network allowlist/);
    // Nothing to name is not a reason to say "add  to the network allowlist".
    const none = netPermissionHint(new Error("Requires net access"), []);
    assert.match(String(none), /add the host it needs to the network allowlist/);
});
