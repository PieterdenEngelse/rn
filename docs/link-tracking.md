# Link tracking through Gmail

Recording that a link was clicked, and reading the links out of mail that
arrives. Both halves, because they are usually wanted together and they cost
nothing like the same amount.

**Nothing here has been measured.** `docs/tunnel.md` is a record of things that
were tried against a real public URL; this is not that document. It is the
`docs/jobs.md` kind: what the surrounding code has already decided, what the
one structural decision is, and where the cost actually sits. Every number
below about Gmail's behaviour is a claim to check, not a result.

The short version, so it cannot be skipped: **the inbound half is an ordinary
rn job and mostly already built for. The outbound half is one email job plus a
public, unauthenticated GET endpoint on a machine whose entire security
position is that nothing is reachable.** Those are not two features of one
size.

---

## 1. How rn talks to Gmail

Everything downstream depends on this and the two paths are not close.

### App password over SMTP and IMAP — recommended

A Google app password (the account needs 2-step verification on) is one opaque
string, which is exactly the shape `be/src/secrets.ts` already carries:

    ~/.config/rn/credentials  →  RN_SECRET_GMAIL_APP_PASSWORD
                              →  ctx.secret("gmailAppPassword")

Redaction is armed the moment it is saved — `credentials-file.ts` applies to
`process.env` before it writes the file, for that reason — so a job that logs
its own auth string has it scrubbed out of the run record before disk. No cloud
project, no consent screen, no refresh, no expiry. Two pure-JS dependencies:
something to send (`nodemailer`) and something to read (`imapflow`). Neither is
a native addon, which `noAddons` would otherwise have decided for us.

### OAuth and the Gmail API

A Google Cloud project, an OAuth client, a consent screen, and a loopback
redirect flow run once to mint a refresh token. The token itself has a home —
`credentials.set()` in `be/src/credentials-file.ts:216` does env-first,
file-second, so a rotated token survives a restart without a person retyping
it. That part is fine.

The paperwork is not. `gmail.readonly`, `gmail.modify` and `gmail.metadata` are
Google's *restricted* scopes, which for anything published means verification
plus a third-party security assessment; and a refresh token issued while the
OAuth app is in Testing status expires in about a week. So the read half either
stalls every seven days or drags a personal tool through an app review. Check
the current policy before believing this paragraph — it is the part of Google's
platform that moves most — but check it *before* starting, not after the first
token dies.

**Take the app password**, unless you specifically need Gmail push
(`users.watch()` onto a Pub/Sub topic), which is the one capability IMAP cannot
give you at all. See §3 for why push is worse than it sounds here anyway.

---

## 2. Inbound: the half that is already designed for

Reading arriving mail and pulling the links out of it is an ordinary job in
`be/src/jobs/`, and it lands on machinery that exists.

**Poll; do not push.** A `schedule?` on the `Job` and either an IMAP
`SEARCH`/`FETCH` or `history.list` from a stored `historyId`. Push means
Pub/Sub, a GCP topic, and a second public endpoint — the whole of §3's cost
paid again, for latency an inbox does not need.

**Dedupe on message id through `ctx.state.seen()`.** This is precisely what
`watch-feeds.ts` was written to prove out, and its arithmetic transfers without
translation: `SEEN_CAPACITY` is 1000 ids per job, oldest falling off, so
messages-per-run has to stay well inside that or one run pushes out ids the same
run recorded. A quiet inbox at twenty messages a run holds months. A busy one at
fifty cannot dedupe at all — and it fails by re-reporting things silently,
weeks later, with nothing red. Check the product up front and put it on the run
record, the way `watch-feeds` does, rather than leaving it to be discovered.

**Extraction is a scan, not a parse.** Same tradeoff `watch-feeds` states for
its feed reader: find `href` on anchors, unwrap entities, and accept that
adversarial HTML can hide a link from it. An HTML parser is a dependency and a
CVE surface for a job whose output is "here are some URLs".

**Resolving redirects is a separate decision, and a heavier one.** Following a
shortener to its destination means outbound requests to hosts chosen by whoever
mailed you. There is no `netAllowlist` that can express "anywhere a stranger
points", so under Deno it either does not work or the allowlist stops meaning
anything; and a job that fetches attacker-chosen URLs from inside the machine is
an SSRF primitive. Default to recording the link as written.

---

## 3. Outbound: the public endpoint, and what it spends

### What actually changes

Recipients click, so the endpoint cannot be signed, cannot be verified, and
cannot be behind anything. Today `be/src/hooks/server.ts:321` refuses every
request that is not `POST /api/hooks/…`, and that refusal is load-bearing in two
other documents: `docs/network.md` §4 and `docs/sec.md` both argue that the
listener is safe *structurally* rather than by configuration, and
`docs/tunnel.md` proves it with the `GET /api/settings` → **404** rows, which
say the route is absent rather than forbidden.

A tracking redirect is the first thing rn has ever served to an unauthenticated
stranger on purpose. Adding it to 3011 spends that argument outright. So:

**Give the tracker its own port and Funnel a second mapping.**

    tailscale funnel --bg 3011                    # unchanged: hooks, POST-only
    tailscale funnel --bg --set-path=/t 3012      # new: the tracker
    tailscale funnel status

That keeps `hooks/server.ts` one-route-by-construction, keeps the tracking links
on 443 where they look like ordinary links, and makes the two surfaces separable
later. Confirm `--set-path` against the installed `tailscale funnel --help`;
without it, Funnel offers only 443, 8443 and 10000, and a port number inside an
emailed URL reads as phishing to both people and filters.

Whichever way it goes, `docs/network.md` and `docs/sec.md` need editing in the
same change. "The hooks port serves exactly one route" stops being true, and a
security argument that has quietly become false is worse than one that was never
made.

### The destination is never in the URL

`/t/<opaque id>`, looked up in a store, `302` to whatever the store says. Put
the target in a query parameter and you have published an open redirect on an
HTTPS host that carries your machine's name — a phishing relay with your
hostname on it, reachable by anyone who guesses the shape.

HMAC-signing the destination into the link avoids needing a store, and is still
wrong: the target is then in the link, visible in every mail client's status bar
and in every scanner's logs. A store also happens to be the click log, so it is
not an extra thing.

### The store is new, and none of the existing ones fit

- `ctx.state` is a **bounded window** (`SEEN_CAPACITY`, oldest evicted). Links
  that age out stop resolving, which is a broken link in a mail somebody kept.
- Run records are a **log**, not a lookup table, and they are pruned.
- `job-state.json` is per-worktree — `~/cc` writes to `~/.cache/rn-state-cc`.
  A link minted while testing in a worktree does not resolve against `~/rn`'s
  tracker, and the symptom is a 404 on a real recipient's click.

Append-only JSONL under the state directory, or SQLite once the count justifies
it. Whatever it is, it needs a retention answer written down at the same time as
the writer, because it is the first store in rn that grows with traffic from
outside the machine.

### One row per recipient, not per message

A tracked link is `(message, recipient, url)` or there is no answer to "who
clicked". That means the send job renders a different body per recipient, so
there is no single BCC send and no reusing one rendered HTML for a list. Decide
it at the schema, not later.

### The send job itself

An ordinary `Job` registered in `be/src/jobs/index.ts`. Three things it owes:

- **A real dry run.** `DRY_RUN` defaults on, and `docs/jobs.md` §1 is explicit
  that a flag a job accepts and ignores is a decoration rather than a safety
  switch. The no-op path must still render the mail, still rewrite the hrefs,
  and still report *which* links it would have minted.
- **`netAllowlist`.** `smtp.gmail.com` (and `imap.gmail.com` for §2), or under
  Deno the run fails with `Requires net access to "smtp.gmail.com:465"` and
  nothing in that message mentions that an allowlist exists.
  `be/src/jobs/net-permission.ts` turns it into advice; call it with the hosts
  the run was actually going to reach.
- **`PermanentFailure` where it applies.** A rejected credential and a
  malformed address answer identically on the third attempt; a 421 from
  Gmail's SMTP is what the retries are for. `docs/jobs.md` §7 has the test.

### Rewriting

Conservative and boring: `href` on `<a>` only; leave `mailto:`, `tel:` and
in-document anchors alone; leave anything already pointing at the tracker alone
so a re-send does not double-wrap. The plain-text alternative part of a
multipart mail carries the same URLs and is the half people forget — an
untracked plain-text link is a click that silently never happened.

---

## 4. What the repo demands of either half

- **Wire types in `shared/`.** Anything that reaches a page — a links table,
  click counts, per-recipient status — is defined once in `shared/src/`, then
  `cd be && npm run types:build`. `be/test/generated.test.ts` fails on a stale
  `wire.ts`. Closed sets (link state, click source) are enums so the TypeScript
  stays a literal union.
- **Info panels in the same change**, per `CLAUDE.md`, not as a follow-up.
  This feature needs them more than most: a bare "Clicks: 4" is *wrong* without
  the panel explaining that scanners are in that number. See §5 — the panel is
  where the honesty about the measurement lives.
- **`./scripts/check.sh`**, the whole thing, not a subset.
- **`npm install` direction.** `be/node_modules` is a symlink into `~/rn` from
  most worktrees; installing `nodemailer` from `~/cc` deletes the link and
  un-shares that tree for good, with one `npm warn reify` line to say so. Add
  dependencies from `~/rn`.
- **No Rust here.** A handful of sends an hour and a redirect handler is Node's
  work by `CLAUDE.md`'s own one-sentence test, and the sentence is hard to
  write for any part of this. The shape that would change the answer is
  extracting and indexing the *content* of thousands of messages, which is a
  different feature.

---

## 5. What the numbers will lie about

The educational requirement bites hardest here, because a click count looks
like a fact and is not one. It is an inference from a request that cannot be
attributed, and **three of the four failures below let rn report success while
being wrong.** The fourth is a problem with being right.

### Scanners click before people do

When mail lands, several automated systems fetch the URLs inside it before any
human sees them: Google's own link scanning on delivery, Microsoft Defender's
Safe Links — which detonates at delivery *and* re-checks at click time —
Proofpoint and Mimecast URL Defense on corporate recipients, antivirus mail
plugins on the desktop.

Each of those issues a GET to `/t/<id>`, and the tracker cannot tell it from a
person. It is the same HTTP request, and detonation sandboxes increasingly run
a real headless browser, so the user-agent says Chrome and the JavaScript
executes.

What that does to the data is worse than "somewhat inflated":

- A click exists for mail nobody opened, and it arrives **seconds after send** —
  so it reads as the most engaged recipient on the list rather than as noise.
- If the redirect ever does anything besides redirect — marks something read,
  triggers a job — it fires for a robot.
- A gateway that rewrites links itself puts its own infrastructure between the
  send and the recipient, so the click on the record is the gateway's.

Every mitigation fails somewhere specific, which is why they are partial rather
than a solution:

| Mitigation | Where it breaks |
|---|---|
| Discard clicks under N seconds old | Safe Links re-detonates hours later; a recipient watching their inbox is discarded as a robot |
| User-agent filtering | The denylist rots continuously, and sandboxes forge browser UAs |
| Unique-per-recipient rather than raw | Removes "one gateway, five hits"; does not remove "recipient X clicked", which is still false |
| A JS interstitial before counting | Sandboxes run JS, users with JS off are dropped, and the redirect stops being instant |

**The design consequence is the part to act on.** The honest presentation is not
a filtered aggregate but the evidence: *1 click, 0.4s after delivery, UA looked
like Chrome*. A filter that turns that into "0 clicks" has hidden the
uncertainty rather than resolved it, and rn's own rule points the other way.
The panel beside the number exists to say what the number cannot distinguish.

### Open tracking is worthless on Gmail specifically

Gmail does not let the client fetch remote images. It proxies them through
`googleusercontent.com`, fetching server-side, caching, and serving its own
copy. Three consequences, and they point in opposite directions — which is what
makes the metric useless rather than merely noisy:

- **The IP and user-agent are Google's.** No location, no ISP, no device, no
  client.
- **The fetch can happen before a human looks**, on delivery or prefetch.
  Inflation.
- **Once cached, later real opens generate no request at all.** Deflation. The
  first open is overcounted and every one after it is missed.

An open rate therefore measures Google's proxy behaviour from one anonymous
address. This is written down so nobody adds a pixel in six months expecting
otherwise; a click is at least tied to one specific link somebody deliberately
targeted, even with the scanner problem above.

### Rewriting costs deliverability, and fails silently

Visible text saying one thing while the `href` points somewhere unrelated is the
defining shape of a phishing mail, and a scored feature in essentially every
filter. Link rewriting is that shape, on purpose. This setup aggravates it three
ways at once:

- The tracker hostname has **no sending reputation** — a cold start, appearing
  as the target of every link in the mail.
- `.ts.net` is a **shared suffix** across every Tailscale node, so whatever
  reputation accrues is pooled with strangers.
- A **personal Gmail account** whose mail suddenly routes all its links through
  an unrelated host is precisely the signature of a compromised account sending
  spam.

**The failure mode is why this sits in a section about numbers.** Nothing
errors. SMTP returns 250, the send job's record is green, the click store is
written, the Jobs page is clean — and the mail is in spam, so the count is zero.
The tracking system reports complete success while having destroyed the thing it
was built to measure, and "your mail went to spam" is not a symptom anyone
traces back to a link rewriter.

### Consent is not optional

Recording that a named person clicked a specific link at a specific time is
personal data about an identified subject — GDPR, with ePrivacy alongside it.
Bulk mail with a notice and a lawful basis is the ordinary commercial pattern
and is fine. Silently rewriting the links in one-to-one correspondence to log
what a colleague did with your mail is covert surveillance of a correspondent,
and the social cost lands whether or not the legal question is clear here.

Two things follow for the build, which is why this is a section and not a
disclaimer:

- Tracking is **per-send opt-in in the job's input**, not a default the
  rewriter applies to everything.
- The click store needs the **retention answer** §3 asks for. Click logs are
  personal data with a lifetime, not application state that may accumulate
  forever because disk is cheap.

---

## 6. What is undecided

- Whether the tracker is a third listener or a route on 3011 — §3 recommends
  the former and neither has been built.
- Retention for the click store, which nothing else in rn has needed.
- Whether `/t/` links carry a per-recipient id at all, or a per-send id with
  the recipient joined server-side. The first leaks list membership to anyone
  who collects two links; the second cannot survive a forwarded mail.
- Everything in §5, all of which is a claim about Gmail from outside Gmail.
  The first real send is the measurement, and this document should be edited
  to say what it found — the way `docs/tunnel.md` was.

---

## 7. The order to build it

Ten landable steps, where a step is one that ends with `./scripts/check.sh`
green and the tree working. The unit is deliberate: the alternative is a branch
that carries a listener, a store, a job and a page at once, and nothing to
bisect when the click count is wrong.

| # | Step | What it lands |
|---|---|---|
| 1 | Ports and config | The tracker port as `BACKEND_PORT + 2` — 3012, 3022, 3032, 3042, which the ten-apart scheme already has room for — in `scripts/dev-ports.sh`, `config.ts` and `be/.env.example`. One step or a failing test: `be/test/env-example.test.ts` holds the example equal to what `config.ts` reads, in both directions. |
| 2 | The tracker listener | `GET /t/:id` and 404 for everything else, mirroring `hooks/server.ts:321`. Tested as a table the way `docs/tunnel.md` tests the hooks port: an unknown id, an attempt to pass a destination in the query, a non-GET. |
| 3 | The click store | Append, look up, retain. Own module, own tests. |
| 4 | The rewriter | A pure function — HTML in, HTML and minted rows out, plain-text alternative included. No I/O, so the cheapest thing here to get right. |
| 5 | The send job | Per-recipient render, a real dry-run path, `PermanentFailure` classification, declared credentials, `JobInfo`; registered in `jobs/index.ts`. |
| 6 | Wire types and the API | `shared/src/`, `npm run types:build`, read endpoints on the API port. |
| 7 | The page | Route, nav, and the info panels that say what §5 says. |
| 8 | Exposure and docs | The Funnel mapping, and the edits §3 promises to `docs/network.md` and `docs/sec.md`. |
| 9 | The inbound job | IMAP, `seen()` dedupe, the window arithmetic on the run record. |
| 10 | An inbound view | Only if the links need a page rather than the run summary. Skippable. |

Three properties of that order, all of them the reason for it:

**Steps 1–7 are entirely on loopback.** Nothing is publicly reachable until 8,
so minting, rewriting, redirecting, logging and the page can all be built and
watched against `127.0.0.1` with no exposure at all. The one irreversible
decision goes last, after the thing has been seen working.

**Step 8 is the only one git cannot undo.** Everything above it is a revert.
Once links are in somebody's mailbox pointing at a public hostname they are out,
and withdrawing the Funnel mapping does not un-send them — it turns them into
dead links in mail people kept.

**The risk is not spread evenly, and the count of ten flatters that.** 3 and 4
are an afternoon each and hard to get wrong. 2, 5 and 8 carry all of it: the
listener is the new surface, the send job is what touches real recipients, and 8
is the security argument changing.

`nodemailer` and `imapflow` are installed before 5 and 9 respectively, and
**from `~/rn`** — an `npm install` in a worktree deletes the symlink and
un-shares that tree for good, with one `npm warn reify` line to say so.
