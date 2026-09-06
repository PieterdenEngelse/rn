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
like a fact and is not one.

**Scanners click before people do.** Google's own Safe Browsing, and any
corporate gateway on the recipient's side, fetch links on delivery. Raw counts
are inflated, and a "click" can exist for a mail nobody opened. Mitigations,
all partial: discard clicks within a few seconds of send, filter on
user-agent, count unique-per-recipient rather than raw. None of them is
reliable, which is the thing to say in the panel rather than the thing to hide
behind a filter.

**Open tracking is worthless on Gmail specifically**, and worth knowing so
nobody adds it later expecting otherwise: Gmail proxies images through
`googleusercontent` and prefetches them, so a pixel fires on delivery. Clicks
are the only signal in this family worth carrying.

**Rewriting costs deliverability.** Anchor text that disagrees with its `href`,
on a hostname unrelated to the sender, is a textbook spam heuristic. Sending as
yourself, from a `.ts.net` name, to people who know you, this is the failure
most likely to actually cost something — and it fails as "your mail went to
spam", which nobody attributes to a link rewriter.

**Consent is not optional.** Recording who clicked what, and when, is personal
data. Defensible for a newsletter with a notice; not defensible silently in
one-to-one correspondence, and not something an info panel makes legal.

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
