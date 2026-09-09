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

**Poll; do not push — where "push" means the Gmail API's.** A `schedule?` on the
`Job` and either an IMAP `SEARCH`/`FETCH` or `history.list` from a stored
`historyId`. `users.watch()` onto a Pub/Sub topic means a GCP project, a
subscription and a second public endpoint for Google to deliver to — the whole
of §3's cost paid again, for latency an inbox does not need.

**IMAP IDLE is not that, and this document conflated them.** IDLE is one
outbound connection this machine opens and holds: nothing listens, nothing is
exposed, no third party is in the path, and it needs no project and no
subscription. So the sentence above rejects Google's push and says nothing about
IDLE, which `be/src/mail/watcher.ts` now uses — opt-in, read-only, reconnecting
with a backoff, and ringing a doorbell rather than reading anything itself. The
schedule stays as the backstop for whatever arrives while the connection is
down.

**One connection per watched mailbox**, because IMAP idles on a *selected*
mailbox and there is no way to watch two over one socket. They reconnect
independently, and each fires a run scoped to the mailbox that changed — which
is not the default: the run has to be given the mailbox as its input, or it
reads INBOX and finds nothing. That mismatch was silent, and it broke precisely
the configuration this document recommends, since watching a label is the answer
when a Gmail filter skips the inbox.

Three things it cost, all found by talking to a server rather than reasoning:
imapflow waits fifteen seconds of inactivity before entering IDLE, which is
right for a client that issues commands and leaves a fifteen-second blind window
after every reconnect for one that does not; and a burst of deliveries is a
burst of `EXISTS` events, so they are coalesced into one run rather than racing
several reads of the same mailbox.

**Dedupe on message id through `ctx.state.seen()`.** This is precisely what
`watch-feeds.ts` was written to prove out, and its arithmetic transfers without
translation: `SEEN_CAPACITY` is 1000 ids per job, oldest falling off, so
messages-per-run has to stay well inside that or one run pushes out ids the same
run recorded. A quiet inbox at twenty messages a run holds months. A busy one at
fifty cannot dedupe at all — and it fails by re-reporting things silently,
weeks later, with nothing red. Check the product up front and put it on the run
record, the way `watch-feeds` does, rather than leaving it to be discovered.

**Open the mailbox read-only.** An ordinary IMAP `FETCH` of a body sets `\Seen`
as a side effect, so the obvious implementation marks a person's unread mail as
read — a change to something they look at every day, made by an automation
pointed at their inbox to *observe* it. `EXAMINE` rather than `SELECT` makes
that impossible at the protocol level instead of relying on every fetch
remembering `BODY.PEEK`, and it is what makes `effectFree: true` an honest
claim rather than a hopeful one.

**Do not call `download()` while iterating `fetch()`.** IMAP runs one command at
a time on a connection, so a download issued inside the fetch loop waits for the
fetch to finish and the fetch cannot finish until the loop consumes it. Nothing
errors: the run hangs until its timeout, every time, against any real server.
Drain the fetch into an array first. Found by talking to a server and not
findable any other way — the unit tests over the pure parts all passed.

**The sender filter is an install setting, not only a job input.** The
scheduler calls `runJob(job, "schedule")` with no input at all, so an automatic
run uses the job's declared defaults — and a filter that lived only on the run
form would be empty on every scheduled run, which is every run that matters. It
would be worse than absent, because the form implies it is working. So
`RN_MAIL_ALLOWED_SENDERS` carries the standing answer and the form field
overrides it for one run started by hand.

**A sender filter narrows the server's search, and is checked again after.**
Narrowing at the server is what keeps mail from anyone else from being fetched,
scanned, or written to a run record — not fetching is the only way to be sure a
message is not stored. But IMAP's `SEARCH FROM` is a substring match over the
whole `From` header, and the display name is a string the sender chooses: a
message from `evil@attacker.example` calling itself `reports@example.com`
satisfies it. So the parsed envelope address is checked again on arrival, and a
message that passed the server and failed that check is *reported* as
`sender-mismatch` rather than silently dropped. It is the one arrival worth a
person seeing.

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

    tailscale funnel --bg 3011                                       # unchanged: hooks, POST-only
    tailscale funnel --bg --set-path=/t http://127.0.0.1:3012/t      # new: the tracker
    tailscale funnel status

That keeps `hooks/server.ts` one-route-by-construction, keeps the tracking links
on 443 where they look like ordinary links, and makes the two surfaces separable
later. Without `--set-path`, Funnel offers only 443, 8443 and 10000, and a port
number inside an emailed URL reads as phishing to both people and filters.

**Three things about that second line, all of them checked against 1.102.3
rather than assumed.**

*`/t` does outrank the existing `/`.* Tailscale builds no mux: `getServeHandler`
resolves per request, trying the exact path, then `/t/`, then `/t`, then walking
up parents until `/`. So `/t/<id>` finds the tracker before the catch-all, and
the hooks mapping is untouched. This was the open question §6 flagged, and the
answer is yes.

*The target carries `/t` because `--set-path` strips it.* `serve.go` wraps the
handler in `http.StripPrefix(mountPoint, h)`, so a bare `--set-path=/t 3012`
delivers `/<id>` to the tracker and every real click 404s — a mapping that
passes every test in the repo and fails on a recipient's first click, because
nothing here exercises the proxy. Naming the target as a full URL with the path
on it makes `ProxyRequest.SetURL` join `/t` back on. The tracker also accepts
the stripped `/<id>` directly, so the link survives these two behaviours ceasing
to cancel out; `be/src/tracker/server.ts` says why that is worth the four lines.

*There is no tailnet-only rehearsal on 443.* `AllowFunnel` is keyed on
`host:port` and not on path — so **every** handler on a funnel-enabled port is
public, including one added with `tailscale serve` rather than `funnel`. To
rehearse the whole chain privately, mount it on a port Funnel is not enabled
for:

    tailscale serve --https=8443 --set-path=/t http://127.0.0.1:3012/t
    # ... click a link, confirm the arrival lands on Monitor → Links ...
    tailscale serve --https=8443 --set-path=/t off

Whichever way it goes, `docs/network.md` and `docs/sec.md` need editing in the
same change. "The hooks port serves exactly one route" stops being true, and a
security argument that has quietly become false is worse than one that was never
made.

#### What that rehearsal measured — 2026-09-09

Run against a worktree's dev tracker on 3032 rather than the install's 3012, so
the links exercised were a worktree's and the real store was never touched. The
proxy behaviour is the same either way; the store is not, and it is the half
worth keeping separate. Nothing was funnel-enabled at any point: `serve status
--json` had `AllowFunnel` on `laptop.tail1e7abb.ts.net:443` alone, which is the
precondition to check *before* trusting a rehearsal on 8443 rather than after.

| Request through `https://laptop.tail1e7abb.ts.net:8443` | Answer |
|---|---|
| `GET /t/<minted id>` | **302** to the stored URL, HTTP/2, valid certificate |
| `HEAD /t/<minted id>` | **302**, recorded with `method: HEAD` |
| `GET /t/<minted id>?url=https://evil.example` | **302** to the *stored* URL — the query is not read |
| `GET /t/<unminted id>` | 404 |
| `GET /t/api/settings` | 404 — two segments, refused before the store |
| `GET /t/` | 404 |

Every 302 in that table was recorded — id, user agent and method — and reached
`/api/links` and Monitor → Links, so the chain from a public-shaped URL to the
board is whole. The three 404s recorded nothing, which is the other half of it:
a probe cannot put a row in somebody's store.

**What the 302 proves, and what it does not.** It proves the two behaviours
§3 names compose to exactly one `/t`: had `--set-path` stripped without the
target rejoining, `/t/<id>` would have arrived as `/<id>` — which this tracker
also accepts, deliberately — and had neither happened the id would still have
resolved. What a passing fetch cannot separate is which half did what. The
failure it *would* have caught is the one worth catching: a target written
`--set-path=/t 3012`, with no path on it, delivers `/<id>` to a tracker that did
not accept the bare shape, and 404s on a recipient's first click.

**Precedence, measured on a second mount.** The run above could not test the
question §6 actually flagged — whether `/t` outranks the existing `/` — because
a single-handler mount has no catch-all to outrank. Mounting both on 8443
reproduces 443's exact shape, and `tailscale serve status` labels the result
`(tailnet only)` beside the funnel-enabled origin, which is the `AllowFunnel`
check confirmed a second way:

    tailscale serve --bg --https=8443 http://127.0.0.1:3031            # the / catch-all
    tailscale serve --bg --https=8443 --set-path=/t http://127.0.0.1:3032/t

The two listeners answer a 404 differently — hooks `{"ok":false}`, tracker
`Not found` — so each row below says which one was reached rather than only that
something refused:

| Request | Answer | Reached |
|---|---|---|
| `GET /` | 404 `{"ok":false}` | hooks — the catch-all still works |
| `GET /t/` | 404 `Not found` | **the tracker**, for a path it rejects |
| `GET /t/<minted id>` | 302 to the stored URL | **the tracker**, and a `tracker-click` in the log |
| `GET /api/hooks/x` | 404 | hooks, which is POST-only |

So `/t` wins, and it wins on the *mount* rather than on the response: `/t/` is a
404 either way, and it is the tracker's 404. That is `getServeHandler` resolving
per request as §3 describes, observed rather than read.

**`tailscale serve` needs root here, and that is worth keeping.** Without
`sudo` the command parses fine and gets as far as `sending serve config` before
`Access denied: serve config denied`; it then suggests `sudo tailscale set
--operator=$USER` to make the sudo unnecessary. Don't. The same permission
covers `tailscale funnel`, so taking it would make the one command in this
feature that git cannot undo runnable without a password — and that prompt is
the last thing standing between a mistyped port and a public endpoint.

**Without `--bg` the mount is a foreground process** that holds the terminal and
prints what it published; Ctrl+C withdraws it. The `... off` line above is for a
backgrounded one, and running it against a foreground mount is not the teardown.

### The origin is never a name you borrowed

The destination rule below is about what a link points *at*. This one is about
what it points *from*, and it is the same argument one level up: a tracked link
is in somebody's mailbox for as long as they keep the message, so the origin can
only ever be corrected for mail **not yet sent**.

That makes the hostname the most permanent decision in this feature, and the
easiest one to make badly, because the bad answers all work. `laptop.tail1e7abb.ts.net`
resolves, serves a valid certificate, and redirects correctly. It is also not
yours: a `*.ts.net` name follows the machine and the tailnet, so renaming the
laptop, replacing it, or leaving the tailnet kills every link ever sent, all at
once, with nothing left to redirect them to. The same is true of a quick tunnel
hostname or an ngrok name. A loopback base URL fails more honestly — it is dead
for everyone immediately.

**So the origin must be a domain you own, and rn enforces it.**
`be/src/tracker/base-url.ts` classifies the base URL and `rewrite()` refuses to
mint against a bad one *before the first id is generated*, so there is no
half-rewritten body and no orphan row in the store. The problems it names —
`malformed`, `insecure`, `loopback`, `ip-literal`, `port`, `borrowed` — are
reported on Monitor → Links as well, because the moment an operator is looking
at that board is the moment they are deciding what the origin should be.

What it cannot check is whether a domain is *yours*. `borrowed` is a list of
provider suffixes, so a clean verdict means "no known problem" and never
"checked and owned"; a free subdomain from a provider not on the list passes and
should not.

**How to actually have one.** Three shapes. They are ordered here by how
permanent the *name* is, which is the axis this section is about — and that
is not the only axis. "The origin has to be awake" below reorders them, and
the two readings disagree about which one comes first:

1. **A named Cloudflare Tunnel on your own domain.** `cloudflared` on this
   machine, `links.yourdomain.com` routed to `127.0.0.1:3012`. No VPS, no open
   inbound port, a certificate you do not manage, and a hostname that is yours
   permanently. Cloudflare sees the redirect traffic, which is acceptable here
   in a way it would not be for the hooks port: there is no signature to
   invalidate, the destination is a public URL anyway, and the only secret in
   the exchange is which opaque id was fetched. Note that this is a *named*
   tunnel — a `trycloudflare.com` quick tunnel is a borrowed hostname and is
   refused.

   It fixes the name and nothing else. `cloudflared` runs *on this machine*, so
   a click while the machine is asleep reaches a tunnel with no origin behind
   it — see below.

2. **A small host you own, joined to the tailnet.** Caddy or nginx on it,
   `links.yourdomain.com` proxying to `100.x.y.z:3012` over Tailscale. Costs a
   few euros a month and some ops, and buys the strongest property available:
   **this machine gets no public surface at all.** Funnel is not involved, so
   the tracker never spends the argument `docs/network.md` §4 and `docs/sec.md`
   make about the hooks port, and the only thing reachable from the internet is
   a box whose whole job is to forward one path.

   It is also the only one of the three that answers when this laptop does not,
   which is the second property and, on the numbers below, the deciding one.

3. **Funnel on the `.ts.net` name.** Free, no extra infrastructure, and
   permanently borrowed. Defensible when every recipient already knows what rn
   is — an internal circle, your own addresses, a pilot you expect to resend
   from scratch. rn refuses this by default; waiving it is a deliberate act.

   **This is what rn is currently set up for.** Two settings and one mapping:

       RN_TRACKER_BASE_URL=https://laptop.tail1e7abb.ts.net/t
       RN_TRACKER_ACCEPT_BORROWED_HOSTNAME=1

       tailscale funnel --bg --set-path=/t http://127.0.0.1:3012/t

   The waiver accepts exactly one thing. A `.ts.net` origin over http, or with
   a port in it, is still refused — those are different mistakes and this says
   nothing about them, which is why the accepted set is a list rather than a
   boolean. Monitor → Links shows the problem *and* the acceptance rather than
   hiding a signed-off problem, because the decision outlives whoever made it.

   **What it commits to, in one sentence:** if this machine is renamed,
   replaced, or leaves the tailnet, every link already sent stops resolving at
   the same moment and there is nothing to redirect them to. The exit is to
   move to option 1 or 2 *before* that happens — links minted after the move
   are fine, and links minted before it are not recoverable, so the cost of
   changing your mind grows with every send.

Whichever it is, set `RN_TRACKER_BASE_URL=https://links.yourdomain.com/t` and do
not change it afterwards. Two consequences worth stating: the `/t` stays in the
base so the domain can host other things later, and **the domain is now a
dependency of mail already sent** — put it on auto-renew, because letting it
lapse breaks links in messages you no longer control and hands the name to
whoever registers it next.

### The origin has to be awake

The section above is about what the origin *is called*. This one is about
whether anything answers there, and it was found by measuring this machine
rather than by reasoning about it — which is why it arrived after the three
options were already ranked.

A tracked link is not beside the content, it is **in front of** it. The
recipient cannot reach the destination unless the redirector answers, so an
origin that is down does not merely lose a click: the person you mailed gets an
error instead of the thing you sent them. Every mitigation in §5 is about
numbers being wrong. This one is about the mail not working.

**Measured on this machine, 2026-09-09.** Suspended for 25h49m of the preceding
44h53m — 18:29→07:07 and 16:43→05:54, two blocks of about thirteen hours, from
`journalctl -b | grep 'PM: suspend'`. Mail is read in exactly those windows.
Links minted here are therefore dead for roughly half of every day, in the
evening and early morning, which is when somebody actually opens their mail.

That is a property of a laptop and not of Tailscale, so it survives changing
the hostname. Options 1 and 3 both put the redirector on this machine and both
inherit it; only option 2 moves it somewhere that stays up. Read for the name
alone the order is 1, 2, 3. Read for whether the link works when it is clicked,
**option 2 is first and the other two are the same answer**.

Two consequences worth stating plainly. **Nothing should be sent to anyone else
from an origin that sleeps** — a pilot to your own address is fine, because a
dead link at midnight costs you nothing and tells you the same thing a live one
would. And **the fix is not a longer retention or a retry**: the store is on
the machine that is asleep, so there is nothing here to make more patient. The
redirector has to move.

Unmeasured, and worth knowing before it matters: what a click *gets* while the
node is offline has not been checked here. Funnel's edge answers something —
presumably a Tailscale error page rather than a connection refusal — and
whether that reads to a recipient as "broken link" or as "this site is down" is
the difference between them forwarding your mail to someone and not. Check it
by clicking a tracked link with the laptop suspended, before deciding this is
acceptable for a pilot.

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

An ordinary `Job` registered in `be/src/jobs/index.ts`. Four things it owes:

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
- **Idempotency per `(send, recipient)`.** A sent-marker committed *before* the
  SMTP call and checked on entry, so a second attempt resumes rather than
  repeats.

**The fourth is not a refinement, and it is the one this document nearly
missed.** `run.ts` implements retry by re-invoking the job function, and
`be/src/jobs/overrides.ts` lets an install raise `retry.attempts` to ten from a
page — no code change, no review. Every other job in the tree survives that by
accident of being a poller: `watch-feeds` retrying merely re-reads a feed. A
send job is the first thing here that is not idempotent, and the failure is
duplicate mail to real recipients, which is the only failure in this feature
that cannot be taken back. A crash part-way through a fifty-recipient send has
the same shape, so the marker earns itself twice over.

**Two things the plan did not anticipate, found while building it.**

*The send id cannot be generated inside the run.* `run.ts` retries by calling
`run()` again, so an id minted in the function is different on the second
attempt — it would match no marker and mail the whole list twice, which is
precisely the failure the marker exists to prevent, reintroduced by the
mechanism meant to fix it. The id is therefore a hash of the subject, the body
and the sorted recipient list, so two attempts agree by construction. The
consequence reads like a bug and is the protection working: sending identical
mail to the same list twice does nothing the second time, and an explicit
`sendId` is how you say you meant it.

*SMTP status codes are inverted against HTTP.* `permanent.ts` exports
`isPermanentStatus`, where 4xx is permanent because in HTTP it is the sender's
fault. In SMTP, 4xx is a *transient* negative reply — greylisting, a rate limit
— and 5xx is the permanent one. Reusing the HTTP helper here would have retried
the unrecoverable failures and given up on the recoverable ones, which is the
worst available combination. `isPermanentSmtp` is written out separately, and
the reason is in a comment above it so nobody unifies them later.

*A definite refusal is not an unknown outcome.* The first version blocked a
recipient the moment it was attempted and never unblocked them, which quietly
defeated the retry policy for the two failures a send to a real list actually
hits: a greylist and a rate limit. The server answers 421 at `RCPT TO`, before
taking any data — so nothing was delivered, there is no duplicate to prevent,
and skipping that recipient on the retry served nobody. The block is now lifted
on a transient refusal and kept on a permanent one, and "refused" is recorded
separately from "unknown" so only the genuinely unknown outcomes need a human
decision. Found by sending against a local SMTP sink; no unit test would have
asked the question.

*One bad address must not hold the rest of the list.* Throwing inside the loop
left every later recipient unsent, and the only reason the third of three ever
went out was that a retry happened to resume past the failure. Failures are
collected and classified over the whole list at the end — retryable unless
every one of them was permanent.

*And the marker does not live in `ctx.state`*, which was the obvious home.
State writes are staged and committed by the runner only if the run finishes
without throwing — correct for a cursor, and exactly backwards for a marker
whose whole job is to survive a run that threw. `seen()` is also a bounded
window, so a long list would evict its own earliest marks mid-run.
`be/src/tracker/sent.ts` is an append-only file for those two reasons.

Whether `retry` should be overridable at all for a job like this is a question
for that feature rather than this one. `overrides.ts` already refuses it for
`effectFree`, on the grounds that a page must not change what rn *believes*
about a job while the job goes on doing whatever it does — and "this work can
safely be repeated" is the same kind of claim. The marker means link tracking
does not have to wait for that answer.

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
attributed, and **four of the five failures below let rn report success while
being wrong.** The fifth is a problem with being right.

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

### A forwarded mail attributes the click to the wrong person

Only under per-recipient link ids — see §6, where that is a live decision — and
unfixable there. Alice forwards the mail to Bob, Bob clicks, and the record says
Alice, because the id is the only identity in the request and it was minted for
her.

Nothing inside the system can detect it. The click is well-formed, it arrives at
a plausible hour, and it resolves to a link genuinely sent to the person it
names. That makes it the quietest failure in this section: a scanner's click at
least looks wrong once the timestamp is examined, while this one looks exactly
like the thing it is not.

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

## 6. What is undecided, and one thing that no longer is

Four things, and they are not the same kind. **One was a decision and has been
settled by running something**, and it is recorded here rather than dropped.
**One is still a decision**, though a narrower one than it first looked: not a
fork between two designs, but which sends carry identity and what that costs. **Two are gaps**: a number nobody has picked, and a set of claims
nobody has measured.

### Settled: a third listener on its own port

The open question was whether the tracker gets its own port or becomes a route
on 3011. §3 recommends separation, and the recommendation was gated on a fact
about the installed tunnel: Funnel serves 443, 8443 and 10000 only, so two
mappings on 443 need `--set-path`. Without it, separation forces a port number
into every emailed URL, which §5 says reads as phishing to filters and to
people — and at that point a route on 3011 with three documents rewritten
honestly is the better trade, because a security argument is recoverable in
prose and deliverability is not.

**Checked 2026-09-06 on tailscale 1.102.3: `--set-path` is there.**

```
FLAGS
  --set-path value
        Appends the specified path to the base URL for accessing the underlying service
```

So separation wins, and the fallback above stands only as the reasoning for a
machine whose tunnel lacks the flag. What Funnel currently carries is the hooks
port and nothing else:

```
https://laptop.tail1e7abb.ts.net (Funnel on)
|-- / proxy http://127.0.0.1:3011
```

**Settled since, by reading 1.102.3's `ipn/ipnlocal/serve.go`.** `--set-path=/t`
does take precedence over that `/` catch-all: there is no mux, `getServeHandler`
tries the exact path and then walks up parents, so `/t/<id>` finds the tracker
first. Longest-prefix was the expected behaviour and it is the actual one.

**And a second thing the check would not have caught, which matters more.**
`--set-path` *strips* the prefix before proxying, so the mapping this document
first wrote — `--set-path=/t 3012` — would have handed the tracker `/<id>` and
404'd every real click. Nothing in the repository would have failed: the tracker
tests speak to the listener directly and never through a proxy, so the first
evidence would have been a recipient reporting a dead link. §3 carries the
corrected mapping and the two-sided guard.

That is worth keeping as a note about method rather than about Tailscale.
Precedence was the question that got asked because it was the one that sounded
uncertain; stripping was never asked about at all, and it was the one that
would have shipped broken. Reading the source answered both. Running the
command would have answered only the first, and looked like a success.

### Decision: which sends carry identity

This was posed as a fork — per-recipient link ids **or** per-send — and it is
narrower than that. **Both can live in one store.** What has to be decided is
which sends mint which, and what an identified one costs.

**Within a single send there is no having both**, and the reason is mechanical
rather than a design nobody has found yet. Attribution needs the URL to differ
per recipient, because the URL is the only channel from send time to click time:

- a **query parameter** is still the URL, and leaks identically;
- a **fragment** is never sent to the server — recoverable only by an
  interstitial running JS, which is slower, breaks with JS off, and still shows
  the identity in the visible URL;
- a **cookie** needs a prior visit and there is none; the first click is the one
  that matters;
- a **referrer** is usually absent from mail clients and would not name a person
  if it were present;
- an **IP** identifies nobody and is personal data in its own right.

So once a send is identified its hrefs differ per recipient, and the costs below
follow from that rather than from how it was built.

#### The schema carries both

    links: {id, send_id, recipient: string | null, url, minted_at}

`null` is a per-send id — shared by everyone, aggregate only. A value is an
identified one, **minted for that send alone**. One store, one page, one code
path: the rewriter takes a flag, and the page shows a recipient matrix where
identity exists and a total where it does not.

Nullable *is* the decision, which is why it belongs before §7's step 3 rather
than after. What a later change picks is the default, not the shape.

**Default to per-send**, because the direction is not symmetric. Per-send to
identified is a clean forward move: new sends mint identified ids, old rows stay
aggregate, nothing is undone. The reverse is not a migration at all — the links
are already in mailboxes with identity baked into ids that keep resolving, so
stopping affects only sends not yet made. §7 puts the irreversible step last for
this reason; this is the same principle one level down.

#### What an identified send costs, and which parts are fixable

| Cost | Fixable |
|---|---|
| **Correlation across sends** — somebody collecting links builds a profile | **Fully.** Scope ids to one send, and never derive one from a stable recipient key. *Alice clicked in send 12* survives; *Alice clicks everything you send* stops existing. |
| **The comparison leak** — two recipients see different hrefs behind identical anchor text | **No, but bounded.** What leaks is that the mail was individually tracked, not the recipient list — provided the id encodes nothing about the address, which per-send-scoped random ids give for free. |
| **Forgeable identity** — a link pasted into a chat produces clicks attributed to whoever it was minted for | **Partly.** Expire ids after a window, and treat a later click from a wildly different user-agent as suspect rather than as the same person. §5 carries most of it: showing the evidence rather than a bare name degrades honestly when the evidence is strange. |
| **Forwarding misattribution** — Bob clicks, the record says Alice | **No.** Indistinguishable in principle from Alice clicking. §5 says why. |

Three of the four move independently, so the reachable position is: identity
available per send, correlation gone, the leak reduced to *this person tracks
their own mail*, forgery visible rather than silent, and **one irreducible error
the info panel states instead of hiding**.

#### What retention does not fix

An earlier draft offered dropping the recipient column after N days as the
answer to all of this. It bounds what rn knows and nothing else: the distinct
hrefs are already in mailboxes, so the comparison leak and the forged click
outlive any retention window. Retention is about the store, not the artifact —
which is still a gap, for the reason below.

### Gap: retention for the click store

Nothing else in rn grows with traffic from outside the machine, so there is no
precedent here to copy. It needs an answer at the same time as the writer rather
than after it.

### Gap: everything in §5

All of it is a claim about Gmail made from outside Gmail. The first real send is
the measurement, and this document should be edited to say what it found — the
way `docs/tunnel.md` was.

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
| 3 | The click store | Append, look up, retain. Own module, own tests. The recipient column is nullable from the first commit — §6 says why that is the schema decision rather than a later one. |
| 4 | The rewriter | A pure function — HTML in, HTML and minted rows out, plain-text alternative included. No I/O, so the cheapest thing here to get right. |
| 5 | The send job | Per-recipient render, a real dry-run path, `PermanentFailure` classification, declared credentials, `JobInfo`; registered in `jobs/index.ts`. Plus the `(send, recipient)` sent-marker committed before the SMTP call — §3 says why that one is not optional. `rewrite()` already refuses an unfit base URL; the job classifies that throw as `PermanentFailure` (retrying a hostname does not fix it) and passes `allowUnsafeBase` only on a dry run. |
| 6 | Wire types and the API | `shared/src/`, `npm run types:build`, read endpoints on the API port. |
| 7 | The page | Route, nav, and the info panels that say what §5 says. |
| 8 | Exposure and docs | The Funnel mapping, rehearsed on `--https=8443` first because a funnel-enabled port has no private path (§3), and the edits §3 promises to `docs/network.md` and `docs/sec.md`. `--set-path=/t` is confirmed against 1.102.3: it outranks the `/` catch-all, and it strips the prefix, so the target is `http://127.0.0.1:3012/t` and not `3012` — §3 has all three findings. |
| 9 | The inbound job | IMAP, `seen()` dedupe, the window arithmetic on the run record. Read-only mailbox open, which is what makes `effectFree` honest — an ordinary fetch sets `\Seen` and would mark a person's unread mail as read while merely observing it. |
| 10 | An inbound view | Only if the links need a page rather than the run summary. Skippable. |

Steps 1 to 7 are landed, and so are the `docs/network.md` and `docs/sec.md`
edits step 8 owes. **Of step 8 only the mapping itself is left** — the private
rehearsal it was to be preceded by has been run — twice, the second time with
both handlers mounted so that precedence had something to outrank — and §3
records what both found.

That is no longer the last question, though, and step 8 was the wrong place to
look for it. "The origin has to be awake" in §3 is a step 0 that nobody wrote
down: **where the redirector lives** decides whether a link works when it is
clicked, and this machine is asleep for about half of each day. The mapping is
a small reversible act; sending mail through an origin that sleeps is not.

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
