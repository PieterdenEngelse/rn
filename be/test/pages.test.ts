/**
 * The page watcher: what it compares, and what it therefore calls a change.
 *
 * The extraction is the whole job. Everything downstream — the hash, the
 * cursor, the report — is bookkeeping over whatever `visibleText` and
 * `applyIgnores` return, so a change in them is a change in this install's
 * definition of "the page moved". That is exactly the kind of thing that
 * should not need a network or a live site to check.
 *
 * The failures worth catching here are the quiet ones, and they run in both
 * directions. Something left in the text that varies per request — a script
 * body, a comment, a build id — means a change reported every hour on a page
 * nobody touched. Something stripped that should have stayed means a real
 * change that never reports at all, which looks identical to a quiet page.
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import { applyIgnores, applyOnly, pageLabel, visibleText } from "../src/jobs/watch-pages.ts";

test("script and style contents are removed, not just their tags", () => {
    // The one that matters most: a build hash inside a script tag changes on
    // every deploy, and a stripper that removed only the tags would leave it in
    // the compared text and report a change nobody made.
    const html = `
        <html><head>
          <style>.a { color: #fff }</style>
          <script>window.__BUILD__ = "b3f91c";</script>
        </head><body><p>Prices held at £40</p></body></html>`;

    const text = visibleText(html);
    assert.equal(text, "Prices held at £40");
    assert.ok(!text.includes("b3f91c"), "the build id survived into the text");
    assert.ok(!text.includes("color"), "the stylesheet survived into the text");
});

test("comments are removed", () => {
    // Server-rendered comments carry timestamps and cache keys far more often
    // than anybody expects.
    const text = visibleText("<p>Open</p><!-- rendered 14:05:11 -->");
    assert.equal(text, "Open");
});

test("block endings become line breaks, so separate paragraphs stay separate", () => {
    // Without this, "one two" and "onetwo" and "one\ntwo" all collapse
    // together, and the ignore list has no lines to work on.
    assert.equal(visibleText("<p>one</p><p>two</p>"), "one\ntwo");
    assert.equal(visibleText("<li>a</li><li>b</li>"), "a\nb");
    assert.equal(visibleText("first<br>second"), "first\nsecond");
});

test("entities are decoded and whitespace is collapsed", () => {
    // Reflow is not a change: a template that rewraps its output, or indents it
    // differently after an edit elsewhere, must not report.
    assert.equal(visibleText("<p>a &amp;   b\n\n   c</p>"), "a & b c");
    assert.equal(visibleText("<p>&lt;tag&gt; &quot;quoted&quot;</p>"), '<tag> "quoted"');
    assert.equal(visibleText("<p>a&nbsp;b</p>"), "a b");
});

test("an unknown entity is left alone rather than eaten", () => {
    // Half-decoding is worse than not decoding: dropping the ones we do not
    // know would make two different pages compare equal.
    assert.equal(visibleText("<p>&hearts; &amp;</p>"), "&hearts; &");
});

test("reflowing the same content produces the same text", () => {
    // The property the job actually depends on, stated as one assertion: two
    // spellings of one page compare equal, so a deploy that only reformats the
    // HTML is not news.
    const a = visibleText("<div><p>Status: <b>operational</b></p></div>");
    const b = visibleText("<div>\n  <p>Status:\n     <b>operational</b>\n  </p>\n</div>");
    assert.equal(a, b);
});

test("a real edit still comes through", () => {
    // The other direction, and the one a too-eager stripper would break.
    const before = visibleText("<p>Status: operational</p>");
    const after = visibleText("<p>Status: degraded</p>");
    assert.notEqual(before, after);
});

test("ignored substrings drop whole lines, without regard to case", () => {
    const text = "Prices held\nLast updated 14:05\nTerms unchanged";
    assert.equal(applyIgnores(text, ["last updated"]), "Prices held\nTerms unchanged");
    assert.equal(applyIgnores(text, ["LAST UPDATED"]), "Prices held\nTerms unchanged");
});

test("an empty ignore list changes nothing", () => {
    const text = "one\ntwo";
    assert.equal(applyIgnores(text, []), text);
});

test("an ignore entry that matches everything leaves nothing to compare", () => {
    // Not an error, and worth pinning: it is the state where the job reports a
    // page as never changing because it threw the page away. The panel warns
    // about it; this says the behaviour is what the warning describes.
    assert.equal(applyIgnores("alpha\nbeta", ["a"]), "");
});

test("a page's label is its host and path, and survives a URL it cannot parse", () => {
    // The label is the state key, so two spellings of one page must not become
    // two remembered pages — and an unparseable string must still produce
    // something rather than throwing inside the poll.
    assert.equal(pageLabel("https://example.com/status"), "example.com/status");
    assert.equal(pageLabel("https://example.com/status/"), "example.com/status");
    assert.equal(pageLabel("https://example.com/"), "example.com");
    assert.equal(pageLabel("not a url"), "not a url");
});

test("a keep-list narrows to the lines that matter, and nothing else", () => {
    const text = "Home\nStatus: operational\nContact us\nPrice: 40";
    assert.equal(applyOnly(text, ["status:"]), "Status: operational");
    assert.equal(applyOnly(text, ["status:", "price"]), "Status: operational\nPrice: 40");
});

test("an empty keep-list watches the whole page", () => {
    // The default, and the one that must not accidentally mean "nothing".
    const text = "one\ntwo";
    assert.equal(applyOnly(text, []), text);
});

test("a keep-list matching nothing selects nothing, rather than everything", () => {
    // The dangerous direction is the other one: a phrase that matched nothing
    // falling back to the whole page would report every unrelated change as
    // though it were the watched line. Empty is correct, and the job reports
    // it as an empty selection rather than passing over it.
    assert.equal(applyOnly("one\ntwo", ["absent"]), "");
});

test("ignore runs before the keep-list, so a record can use both", () => {
    // "price updated at 14:05" is noise and contains the watched word, so the
    // order is what makes the pair expressible at all.
    const text = "Price: 40\nPrice updated at 14:05";
    const kept = applyOnly(applyIgnores(text, ["updated at"]), ["price"]);
    assert.equal(kept, "Price: 40");
});
