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
