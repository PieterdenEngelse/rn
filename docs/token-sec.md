# Tokens on screen

`docs/sec.md` covers where credentials live and who can read them. `docs/network.md`
covers who can reach the API. This covers the third surface, which is neither:
**what happens when a secret is rendered into the UI.**

It exists because displaying a value is not a read — it is a broadcast. A
credential sitting in a 0600 file has one adversary. The same credential drawn
onto a monitoring page has three, and two of them are not in either of the other
documents' threat models.

---

## The rule

Panels report that a value is **set**. They do not report what it is.

For `be/.env`, that rule has a precise form, implemented in `be/src/env-file.ts`:

- a key rn **recognises** may show its value
- a key rn **does not recognise** is reported by name and as set, never valued

"Recognises" means *appears in `be/.env.example`*. That looks like indirection
and is the opposite: `be/test/env-example.test.ts` holds the example equal, in
both directions, to what `config.ts` actually reads. So the recognised set is
exactly rn's own settings — and a separate test forbids any `RN_SECRET_*` key
from appearing in the example at all. Every recognised key is therefore
non-secret by construction, and the list cannot drift, because adding a setting
to `config.ts` already requires adding it to the example.

Recognised values still pass through `secrets.redact()` on the way out. That is
not redundant: it catches a configured credential that someone *also* put in
`.env` under a name that happens to be recognised.

---

## Why `.env` cannot be vouched for

`.env` is gitignored and user-editable — a file people put things in.
`.env.example` states that credentials belong in `~/.config/rn/credentials`,
outside the install tree, and a test enforces that **for the example**. Nothing
enforces it for `.env` itself, and people paste tokens into dotfiles.

The keys rn does not recognise are exactly the ones it cannot vouch for. Since
you cannot enumerate what a user put there, the only safe treatment of an
unknown key is to assert its existence and nothing else — the same rule
`secrets.describe()` already follows for credentials, for the same reason.

---

## Three ways a rendered secret escapes

The three have very different attacker populations, which is why the exposure is
worse than "it is on a page" suggests.

### The browser

Narrower than usually assumed. rn echoes an `access-control-allow-origin` only
for allowlisted origins and otherwise sends the first entry, so a page you
happen to have open cannot read the response — the browser discards it. DNS
rebinding does not defeat this either, since the origin stays the attacker's.

What *does* defeat it: **browser extensions with host permissions**, which
bypass CORS entirely and sit in a tab left open all day. A monitoring page is a
uniquely good target for one, because it is the tab nobody closes.

### Screenshots

The sneakiest path, because it defeats the tooling built to catch exactly this.
`gitleaks`, GitHub push protection and every pre-commit secret scanner read
**text**. A token rendered into a PNG is invisible to all of them: a value that
would be blocked on `git push` sails through as an image.

Screenshots also travel further than files do — into issues, chat, bug reports
and model conversations. This project's own convention is "look at it"
(`CLAUDE.md`, *Checking the page*), so screenshots of these pages are routine
and are produced in bulk. That convention is worth keeping and is precisely why
the panels must be safe to photograph.

### The API, which has no authentication

The widest by far. `be/src/server.ts` has no bearer, no key, no auth check of
any kind. The bind address *is* the bearer capability: anyone holding it is
authorised, and `remoteBindRefusal` exists so binding a routable one takes an
explicit `RN_ALLOW_REMOTE=1`.

Loopback is a control against remote attackers and **no control at all** against
local ones. On a developer machine "local" is a large and mostly untrusted set:

- every npm/pnpm `postinstall` script — arbitrary code, at install time
- every editor extension
- every `build.rs` in a cargo dependency
- anything else running as you

None of them need a browser, an origin, or CORS. They open the port and read the
response. `docs/sec.md` makes the same point about *driving* the API — the
confused deputy that runs a job for anyone who asks — and this is its reading
half: a panel that renders a secret hands it to every one of those processes,
without their needing to know the secret existed.

---

## What a stolen token buys

Blast radius depends on the token, and rarely stops at the service it belongs
to.

| Token | Escalation |
|---|---|
| Mail app password | Read the mailbox → drive password resets elsewhere → account-takeover chain. Also sends *as you*, from an address people trust |
| Version-control PAT | Private repo read; with write scope, push → supply chain; read CI secrets |
| Cloud API key | Data, spend, and lateral movement into whatever the key's role can reach |
| Webhook signing secret | Not data access — execution. See below |

### The signing secret is a different kind of loss

Every other row is a read. A leaked webhook signing secret is *execution*: a
correctly-signed delivery to the hooks port makes rn run a job on the attacker's
command, with `DRY_RUN` off, holding whatever credentials that job declares.

It converts a disclosure into a foothold in the automation surface, and the
resulting run looks entirely legitimate in the record — which is the property
that makes it hard to notice afterwards. `docs/sec.md`, *What protects a
delivery*, covers the signature scheme itself.

---

## What follows for anyone adding a panel

1. **Render existence, not content**, for anything you did not define. `set` /
   `not set` is almost always the whole useful signal.
2. **Derive the vocabulary from something a test enforces**, rather than
   maintaining a third list of "safe keys" that drifts. `env-file.ts` borrows
   `.env.example` for exactly this reason.
3. **Redact on the way out anyway.** The two rules catch different mistakes.
4. **Assume the panel will be photographed.** It will be.

---

## The write direction

Config → Jobs has a credentials board. It is the one place in rn where a secret
is typed into a browser, so it belongs in this document rather than in a commit
message.

**It takes values and never gives one back.** No masked form, no prefix, no
length — the same rule as everything above, and enforced the blunt way in
`be/test/credentials-file.test.ts`: a distinctive value is saved, then every
shape the API can hand a page is serialised and asserted not to contain it, or
its first eight characters. The `<input>` is `type="password"` with no `value`
binding at all, so there is nothing for a repaint to put back.

**Why writing is not the same question as reading.** The API has no
authentication, so the test for any new endpoint is what a local process gains
— and "local" here is the list above: postinstall scripts, editor extensions,
`build.rs`. It gains nothing. That process runs as the user, so it can already
open `~/.config/rn/credentials` with `fs` and write it, and it can already
`POST /api/jobs/:id` to run any automation holding any credential without ever
learning one. A *read* endpoint would hand it something it did not have. This
one does not. What would change the calculus is the API on a routable address,
which `remoteBindRefusal` and `RN_ALLOW_REMOTE` already make deliberate.

**A save applies to the process before it touches the disk**, and the order is
the interesting part. `secrets.redact()` finds values by scanning `RN_SECRET_*`
at call time, so a credential in the file but not yet in the environment is one
rn does not know to scrub. Writing the environment first closes the window in
which a run finishing a millisecond later could put the new value into a record.
It also means a saved credential works immediately, with no restart to tell
anyone about.

**Errors never quote the value.** An error message is written to a log and
rendered on a page, which are the two places this document exists to keep a
credential out of, so "expected X, got Y" is the tempting mistake and there is a
test against it. The only fact about a value that may be stated is its length,
and only when it is already over a published cap.

---

## Metadata about a secret, which is not the secret

Monitor → Connection carries a Tokens board, and it exists because of the gap
this document leaves: the rule above keeps a value off the screen, and says
nothing about *when the value stops working*. An expired token reads as set
everywhere in rn, and the first evidence is a job failing at whatever hour it
expired — a 401 in one job's log, with nothing naming the cause.

What crosses the boundary there, and why each is not a value:

- **An expiry instant.** Read inside the backend from the token's own `exp`
  claim; only the timestamp is sent. Not the claim set, not the issuer, not a
  prefix. A JWT carries one; a personal access token or an app password does
  not, and the board says which rather than leaving a blank that reads as
  "fine".
- **Job ids that declare it.** Already public on Config → Jobs — this is the
  same list, read for a different question: what stops when the credential
  does.
- **When a job using it last succeeded**, and **how many recent failures read
  as a refusal**, with the failure's message. Messages go out through
  `secrets.redact` like every other error.

Nothing on that board calls a provider. It is derived from the token this
process already holds and from runs that already happened, so an open page is
not traffic and cannot exhaust a rate limit — and a read endpoint that hands a
local process nothing it did not already have stays within the argument in
`shared/src/credentials.rs`.

**The probe is the one thing there that uses a value.** "Works now" asks the
provider, from the backend, with the secret never leaving the process: what
crosses the boundary is `ok` and one short line of the provider's own words,
redacted like any other message. It is a button and never a poll — a page left
open would otherwise spend a token's rate limit all day — and the call is
chosen to be the cheapest the provider has: GitHub's `/rate_limit`, which
GitHub exempts from the limit it reports, and for a mailbox a login followed
immediately by a logout, opening nothing.

A credential nothing can probe says so. An inbound signing secret has no
outward endpoint that would accept it, and an empty cell there would read as
untested rather than untestable.

## What this does not protect against

Stated plainly, on the same principle as the equivalent section in `docs/sec.md`.

- **A job that sends a credential somewhere on purpose.** Nothing here touches
  what a job *does*, only what rn *displays*.
- **Anything already on screen.** A value that has been rendered — into a
  browser, a screenshot, or an API response — should be treated as disclosed and
  rotated. Redaction applies to what is written next, not to what was written
  before.
- **The process environment.** Credentials reach the backend through its
  environment and are visible in `/proc/<pid>/environ` to the same user. That is
  a storage-and-access question, and it belongs to `docs/sec.md`.
- **A recognised key holding something it should not.** `secrets.redact()`
  catches configured credentials; it cannot catch a secret that was never
  configured anywhere.

---

## See also

- `docs/sec.md` — where credentials live, redaction, and the storage threat model
- `docs/network.md` — who can reach the API, and the remote-bind refusal
- `be/src/env-file.ts` — the implementation, with the reasoning in its module doc
- `be/test/env-example.test.ts` — the test the recognised-key vocabulary rests on
