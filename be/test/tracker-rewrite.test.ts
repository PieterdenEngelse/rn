/**
 * The rewriter: what it changes, and the much longer list of what it does not.
 *
 * This is the one module in the feature whose output goes straight into a
 * message somebody sends to a person, so its failures are not statistical. A
 * link it misses costs a number on a page. A link it mangles is a mail that
 * arrives broken, and there is no undo for that once it is sent — which is why
 * the "leaves it alone" cases outnumber the rewriting ones here.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { rewrite } from "../src/tracker/rewrite.ts";

const BASE = "https://example.ts.net/t";

/** Deterministic ids, so the assertions can name them. */
function counter() {
    let n = 0;
    return () => `id${++n}`;
}

test("an anchor href is rewritten and the link is reported", () => {
    const out = rewrite(
        '<p>See <a href="https://example.com/a">this</a>.</p>',
        "",
        BASE,
        counter(),
    );
    assert.equal(out.html, '<p>See <a href="https://example.ts.net/t/id1">this</a>.</p>');
    assert.deepEqual(out.minted, [{ id: "id1", url: "https://example.com/a" }]);
});

test("the plain-text alternative is rewritten too", () => {
    // The half people forget. An untracked text link is a click that silently
    // never happened, and the two bodies then disagree about where the mail
    // points — which is a thing spam filters look at.
    const out = rewrite("", "See https://example.com/a for more", BASE, counter());
    assert.equal(out.text, "See https://example.ts.net/t/id1 for more");
});

test("a link at the end of a sentence keeps its punctuation outside the url", () => {
    const out = rewrite("", "Read https://example.com/a. Then stop.", BASE, counter());
    assert.equal(out.text, "Read https://example.ts.net/t/id1. Then stop.");
    assert.deepEqual(out.minted, [{ id: "id1", url: "https://example.com/a" }]);
});

test("a link inside parentheses does not swallow the closing paren", () => {
    const out = rewrite("", "(see https://example.com/a)", BASE, counter());
    assert.equal(out.text, "(see https://example.ts.net/t/id1)");
});

test("single quotes are handled and the quote style is preserved", () => {
    const out = rewrite("<a href='https://example.com/a'>x</a>", "", BASE, counter());
    assert.equal(out.html, "<a href='https://example.ts.net/t/id1'>x</a>");
});

test("other attributes and whitespace on the anchor survive untouched", () => {
    const out = rewrite(
        '<a class="btn" href = "https://example.com/a" target="_blank">x</a>',
        "",
        BASE,
        counter(),
    );
    assert.equal(
        out.html,
        '<a class="btn" href = "https://example.ts.net/t/id1" target="_blank">x</a>',
    );
});

test("mailto, tel and in-document anchors are left exactly as they were", () => {
    // Rewriting one of these does not lose a statistic, it breaks the link.
    const html = [
        '<a href="mailto:someone@example.com">mail</a>',
        '<a href="tel:+3112345678">call</a>',
        '<a href="#section">jump</a>',
        '<a href="/relative/path">relative</a>',
    ].join("");
    const out = rewrite(html, "", BASE, counter());
    assert.equal(out.html, html);
    assert.deepEqual(out.minted, []);
});

test("a javascript href is not minted a redirect", () => {
    // Somebody else's problem in a mail body; minting a link that redirects to
    // it would make it rn's, on rn's hostname.
    const html = '<a href="javascript:alert(1)">x</a>';
    assert.equal(rewrite(html, "", BASE, counter()).html, html);
});

test("an already-tracked link is not wrapped again", () => {
    // Double-wrapping still resolves, which is what makes it dangerous: the
    // inner click is recorded against whoever the outer id was minted for, so
    // a re-send would produce a wrong answer that looks like a right one.
    const html = `<a href="${BASE}/id1">x</a>`;
    const out = rewrite(html, "", BASE, counter());
    assert.equal(out.html, html);
    assert.deepEqual(out.minted, []);
});

test("the same url in two places is minted twice", () => {
    // Two anchors are two places a person can click. Collapsing them would make
    // the report unable to say which one was used.
    const out = rewrite(
        '<a href="https://example.com/a">one</a><a href="https://example.com/a">two</a>',
        "",
        BASE,
        counter(),
    );
    assert.equal(out.minted.length, 2);
    assert.notEqual(out.minted[0]!.id, out.minted[1]!.id);
});

test("markup that is not an anchor href is never touched", () => {
    // The scan's safe direction: a link it misses costs a number, markup it
    // mangles costs a mail. Images, link tags and bare text stay as they are.
    const html = [
        '<img src="https://example.com/pixel.png">',
        '<link rel="stylesheet" href="https://example.com/style.css">',
        "<p>https://example.com/plain-text-in-html</p>",
    ].join("");
    const out = rewrite(html, "", BASE, counter());
    assert.equal(out.html, html);
    assert.deepEqual(out.minted, []);
});

test("a body with no links comes back unchanged and mints nothing", () => {
    const out = rewrite("<p>Hello</p>", "Hello", BASE, counter());
    assert.equal(out.html, "<p>Hello</p>");
    assert.equal(out.text, "Hello");
    assert.deepEqual(out.minted, []);
});
