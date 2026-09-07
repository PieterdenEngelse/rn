/**
 * What may be minted into mail, and what may not.
 *
 * The table below is the one place the permanence argument is machine-checked.
 * Every other test in this feature protects a behaviour that a revert could
 * fix; these protect the one decision that a revert cannot reach, because the
 * artefact is in somebody else's mailbox.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyBaseUrl, assertMintableBase } from "../src/tracker/base-url.ts";

test("a domain you own with https and no port is clean", () => {
    for (const base of [
        "https://links.example.com/t",
        "https://example.co.uk/t",
        "https://tracking.example.org/some/prefix",
    ]) {
        assert.deepEqual(classifyBaseUrl(base), [], base);
    }
});

test("each problem is detected on its own", () => {
    const cases: [string, string[]][] = [
        ["https://links.example.com/t", []],
        ["http://links.example.com/t", ["insecure"]],
        ["https://localhost/t", ["loopback"]],
        ["https://127.0.0.1/t", ["loopback"]],
        ["https://[::1]/t", ["loopback"]],
        ["https://93.184.216.34/t", ["ip-literal"]],
        ["https://laptop.tail1e7abb.ts.net/t", ["borrowed"]],
        ["https://random-words.trycloudflare.com/t", ["borrowed"]],
        ["https://abc123.ngrok-free.app/t", ["borrowed"]],
        ["https://links.example.com:8443/t", ["port"]],
        ["not a url at all", ["malformed"]],
        ["ftp://links.example.com/t", ["malformed"]],
    ];

    for (const [base, expected] of cases) {
        assert.deepEqual(classifyBaseUrl(base), expected, base);
    }
});

test("the shipped default is not fit to mint, and says so three times over", () => {
    // Reported as a list rather than a first offence: an operator who fixed
    // only the scheme would see the same refusal again, and the point is to
    // move them to a real origin in one step rather than three.
    assert.deepEqual(classifyBaseUrl("http://127.0.0.1:3012/t"), [
        "insecure",
        "loopback",
        "port",
    ]);
});

test("a named tunnel on your own domain is clean; the throwaway one is not", () => {
    // The distinction worth keeping: Cloudflare is not the problem, and a
    // named tunnel fronted by a domain you own is exactly the recommended
    // shape. It is the generated hostname that cannot be kept.
    assert.deepEqual(classifyBaseUrl("https://links.example.com/t"), []);
    assert.deepEqual(classifyBaseUrl("https://brave-tiger-runs.trycloudflare.com/t"), [
        "borrowed",
    ]);
});

test("the refusal names every problem and can be waived deliberately", () => {
    assert.throws(() => assertMintableBase("http://127.0.0.1:3012/t"), /insecure, loopback, port/);
    assert.doesNotThrow(() => assertMintableBase("http://127.0.0.1:3012/t", true));
    assert.doesNotThrow(() => assertMintableBase("https://links.example.com/t"));
});

test("a clean verdict is not a claim that the domain is yours", () => {
    // Stated as a test because it is the limit of the check and the easiest
    // thing to forget: a free subdomain from a provider not on the list looks
    // exactly like a domain somebody bought.
    assert.deepEqual(classifyBaseUrl("https://someones-free-subdomain.example.net/t"), []);
});
