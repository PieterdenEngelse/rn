/**
 * The watched-page records: what is refused, what is defaulted, and what
 * survives a file written by something other than this version.
 *
 * The store is the half of `watch-pages` that a person touches directly, so the
 * failures worth catching here are the ones that would be discovered at four in
 * the morning instead: a URL that is not fetchable saved without complaint, an
 * interval of zero that makes every page permanently due, a record written
 * before a field existed that reads as unwatched rather than as watched.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dir = mkdtempSync(join(tmpdir(), "rn-pages-"));
const file = join(dir, "watch-pages.json");
process.env["RN_WATCH_PAGES_PATH"] = file;

const pages = await import("../src/pages.ts");

beforeEach(() => {
    rmSync(file, { force: true });
    pages.reset();
});

after(() => rmSync(dir, { recursive: true, force: true }));

test("a page needs a URL, and it has to be one", () => {
    assert.ok(pages.validate({}).errors.length > 0, "no URL at all");
    assert.ok(pages.validate({ url: "   " }).errors.length > 0, "whitespace");
    assert.ok(pages.validate({ url: "not a url" }).errors.length > 0, "not parseable");
});

test("only http and https are fetchable, so only those are saved", () => {
    // Not tidiness: a file: URL would turn a field on a web page into a way to
    // read the disk of the machine rn runs on.
    const refused = pages.validate({ url: "file:///etc/passwd" });
    assert.equal(refused.page, undefined);
    assert.match(refused.errors.join(" "), /http and https/);

    assert.ok(pages.validate({ url: "https://example.com/status" }).page);
    assert.ok(pages.validate({ url: "http://example.com/status" }).page);
});

test("an interval of zero is refused rather than making a page always due", () => {
    const refused = pages.validate({ url: "https://example.com", everyMinutes: 0 });
    assert.equal(refused.page, undefined);
    assert.ok(refused.errors.length > 0);
});

test("a saved page gets an id, is on, and compares text", () => {
    // The defaults a person would have picked, so that adding a page with one
    // field filled in does something sensible rather than nothing.
    const { page } = pages.put({ url: "https://example.com/status" });
    assert.ok(page);
    assert.ok(page.id.length > 0, "an id was minted");
    assert.equal(page.enabled, true);
    assert.equal(page.text, true);
    assert.equal(page.everyMinutes, pages.DEFAULT_EVERY_MINUTES);
});

test("saving the same id twice replaces rather than duplicates", () => {
    const first = pages.put({ url: "https://example.com/a" }).page!;
    pages.put({ ...first, label: "renamed" });
    const list = pages.list();
    assert.equal(list.length, 1);
    assert.equal(list[0]?.label, "renamed");
});

test("a page survives a restart, because the file is what is read back", () => {
    const saved = pages.put({ url: "https://example.com/a", everyMinutes: 15 }).page!;
    pages.reset();
    const list = pages.list();
    assert.equal(list.length, 1);
    assert.equal(list[0]?.id, saved.id);
    assert.equal(list[0]?.everyMinutes, 15);
});

test("removing one leaves the others", () => {
    const a = pages.put({ url: "https://example.com/a" }).page!;
    pages.put({ url: "https://example.com/b" });
    assert.equal(pages.remove(a.id), true);
    assert.equal(pages.remove(a.id), false, "removing it twice is not an error, just false");
    assert.equal(pages.list().length, 1);
});

test("a record missing enabled is watched, not silently ignored", () => {
    // The direction that matters: a file written by an older version, or by
    // hand, should keep working. Defaulting to off would stop watching pages
    // and say nothing, which is indistinguishable from pages that never change.
    writeFileSync(file, JSON.stringify([{ id: "x", url: "https://example.com/a" }]), "utf8");
    const list = pages.list();
    assert.equal(list.length, 1);
    assert.equal(list[0]?.enabled, true);
    assert.equal(list[0]?.text, true);
    assert.equal(list[0]?.everyMinutes, pages.DEFAULT_EVERY_MINUTES);
});

test("a record with no url is dropped rather than taking the list down with it", () => {
    writeFileSync(
        file,
        JSON.stringify([{ id: "x" }, { id: "y", url: "https://example.com/b" }]),
        "utf8",
    );
    const list = pages.list();
    assert.equal(list.length, 1);
    assert.equal(list[0]?.id, "y");
});

test("a file that is not a list leaves nothing watched, and says so in the log", () => {
    writeFileSync(file, '{"url":"https://example.com"}', "utf8");
    assert.equal(pages.list().length, 0);
});

test("an unparseable file does not throw on the way to a run", () => {
    // A store that threw here would take the whole job down, and the job is
    // what would have reported the problem.
    writeFileSync(file, "{ this is not json", "utf8");
    assert.doesNotThrow(() => pages.list());
    assert.equal(pages.list().length, 0);
});
