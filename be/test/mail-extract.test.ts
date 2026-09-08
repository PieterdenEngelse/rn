/**
 * Pulling links out of mail that arrived.
 *
 * The scan is the whole of what the read-mail job reports, so what it finds and
 * what it deliberately does not are both worth pinning. The cases that earn
 * their place are the ones where being wrong is silent: an entity left encoded
 * produces a URL that is subtly not the one in the message, and a link counted
 * twice inflates a number somebody will read as evidence.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import {
    extractLinks,
    messageKey,
    fallbackKey,
    senderMatches,
    recipientMatches,
    parseSenders,
} from "../src/mail/extract.ts";
import { textPartNumbers } from "../src/jobs/read-mail.ts";

test("anchors in html and bare urls in text are both found", () => {
    const links = extractLinks(
        '<p>See <a href="https://example.com/a">this</a>.</p>',
        "and also https://example.com/b",
    );
    assert.deepEqual(links, ["https://example.com/a", "https://example.com/b"]);
});

test("the same url in both parts is one link, not two", () => {
    // The opposite of the rule the outbound rewriter follows, and deliberately
    // so: there, two anchors are two places a person can click. Here the parts
    // are two renderings of one message and the reader clicks once.
    const links = extractLinks(
        '<a href="https://example.com/a">a</a>',
        "see https://example.com/a",
    );
    assert.deepEqual(links, ["https://example.com/a"]);
});

test("&amp; in a query string is decoded", () => {
    // The one that matters. A two-parameter query is written a=1&amp;b=2 in
    // valid HTML, and leaving it encoded records a URL that is subtly not the
    // one in the message — which nothing downstream would ever flag.
    const links = extractLinks('<a href="https://example.com/x?a=1&amp;b=2">x</a>', "");
    assert.deepEqual(links, ["https://example.com/x?a=1&b=2"]);
});

test("a link at the end of a sentence keeps its full stop out of the url", () => {
    assert.deepEqual(extractLinks("", "read https://example.com/a."), [
        "https://example.com/a",
    ]);
    assert.deepEqual(extractLinks("", "(see https://example.com/a)"), [
        "https://example.com/a",
    ]);
});

test("mailto, tel and javascript are not links to report", () => {
    const links = extractLinks(
        '<a href="mailto:x@y.com">m</a><a href="tel:+3112345">t</a>' +
            '<a href="javascript:alert(1)">j</a><a href="#top">a</a>',
        "",
    );
    assert.deepEqual(links, []);
});

test("single quotes and extra attributes do not hide a link", () => {
    const links = extractLinks(
        "<a class='btn' href='https://example.com/a' target='_blank'>x</a>",
        "",
    );
    assert.deepEqual(links, ["https://example.com/a"]);
});

test("a shortener is recorded as the shortener", () => {
    // Never resolved. Following it would mean requests to hosts chosen by
    // whoever mailed you, from inside this machine — an SSRF primitive that no
    // netAllowlist can bound. docs/link-tracking.md §2.
    assert.deepEqual(extractLinks('<a href="https://bit.ly/3xYz">x</a>', ""), [
        "https://bit.ly/3xYz",
    ]);
});

test("the dedupe key is qualified by mailbox", () => {
    // Gmail's labels are folders over one store, so the same Message-ID is in
    // INBOX and in every label on it. Unqualified, polling a second mailbox
    // would report nothing, having already seen everything in it.
    assert.notEqual(messageKey("INBOX", "<a@b>"), messageKey("rn/reports", "<a@b>"));
    assert.equal(messageKey("INBOX", "<a@b>"), messageKey("INBOX", "<a@b>"));
    // Hashed, so a Message-ID chosen by a sender cannot decide the id length.
    assert.match(messageKey("INBOX", "<a@b>"), /^[0-9a-f]{32}$/);
});

test("a message with no Message-ID gets a key that survives a mailbox rebuild", () => {
    // The header is a should, not a must, and bulk senders omit it. A UID alone
    // would be wrong: UIDs are unique per UIDVALIDITY generation, so a
    // recreated mailbox reissues them from 1 and every old id would collide.
    assert.notEqual(fallbackKey(1, "2026-01-01", "Hello"), fallbackKey(1, "2026-06-01", "Hello"));
    assert.notEqual(fallbackKey(1, "2026-01-01", "Hello"), fallbackKey(1, "2026-01-01", "Other"));
});

test("the text parts of a multipart/alternative are found", () => {
    const parts = textPartNumbers({
        type: "multipart/alternative",
        childNodes: [
            { part: "1", type: "text/plain" },
            { part: "2", type: "text/html" },
        ],
    });
    assert.deepEqual(parts, { text: "1", html: "2" });
});

test("a single-part message has no part number and is still readable", () => {
    // IMAP calls the whole message "1" when there is nothing to enumerate, and
    // imapflow reports no `part` at all — so a naive walk finds the type and
    // has nothing to download.
    assert.deepEqual(textPartNumbers({ type: "text/plain" }), { text: "1" });
});

test("text parts nested under an attachment wrapper are still found", () => {
    // multipart/mixed wrapping multipart/alternative wrapping the two text
    // parts is what any mail with an attachment looks like.
    const parts = textPartNumbers({
        type: "multipart/mixed",
        childNodes: [
            {
                part: "1",
                type: "multipart/alternative",
                childNodes: [
                    { part: "1.1", type: "text/plain" },
                    { part: "1.2", type: "text/html" },
                ],
            },
            { part: "2", type: "application/pdf" },
        ],
    });
    assert.deepEqual(parts, { text: "1.1", html: "1.2" });
});

test("an exact address matches only itself", () => {
    assert.equal(senderMatches("reports@example.com", ["reports@example.com"]), true);
    assert.equal(senderMatches("other@example.com", ["reports@example.com"]), false);
    // Case is not the sender's to decide.
    assert.equal(senderMatches("Reports@Example.COM", ["reports@example.com"]), true);
});

test("a bare domain matches anybody at it, either spelling", () => {
    for (const pattern of ["example.com", "@example.com"]) {
        assert.equal(senderMatches("anyone@example.com", [pattern]), true, pattern);
        assert.equal(senderMatches("anyone@other.com", [pattern]), false, pattern);
    }
});

test("a domain filter is not a substring match", () => {
    // The obvious wrong implementation. `notexample.com` contains
    // `example.com`, and an attacker can register it.
    assert.equal(senderMatches("someone@notexample.com", ["example.com"]), false);
    assert.equal(senderMatches("someone@example.com.evil.test", ["example.com"]), false);
});

test("a display name cannot impersonate a sender", () => {
    // The reason this check exists at all. IMAP's SEARCH FROM is a substring
    // match over the whole header, and the display name is chosen by the
    // sender — so this message satisfies the server-side search for
    // example.com. It must not satisfy the filter.
    assert.equal(senderMatches("evil@attacker.example", ["example.com"]), false);
    assert.equal(senderMatches("evil@attacker.example", ["reports@example.com"]), false);
});

test("no patterns means no filtering, and no address means no match", () => {
    assert.equal(senderMatches("anyone@example.com", []), true);
    // A message whose envelope carries no parsed address cannot be shown to
    // come from an allowed sender, so it does not pass a filter that exists.
    assert.equal(senderMatches("", ["example.com"]), false);
    assert.equal(senderMatches("", []), true);
});

test("senders are split on lines, commas and semicolons", () => {
    assert.deepEqual(parseSenders("a@x.com, b@y.com\n@z.com; "), [
        "a@x.com",
        "b@y.com",
        "@z.com",
    ]);
    assert.deepEqual(parseSenders("   "), []);
});

test("a recipient filter matches To or Cc, and nothing else", () => {
    const to = ["her@example.com", "someone@other.test"];
    assert.equal(recipientMatches(to, ["her@example.com"]), true);
    assert.equal(recipientMatches(to, ["other.test"]), true);
    assert.equal(recipientMatches(to, ["nobody@example.com"]), false);
    // Empty patterns is "no filtering", the same convention as senders.
    assert.equal(recipientMatches(to, []), true);
    // A message with no parsed recipients cannot satisfy a filter that exists.
    assert.equal(recipientMatches([], ["her@example.com"]), false);
});

test("a recipient display name cannot impersonate an address", () => {
    // Same hazard as the sender side: IMAP's SEARCH TO is a substring match
    // over the header, display names included.
    assert.equal(recipientMatches(["evil@attacker.example"], ["example.com"]), false);
});
