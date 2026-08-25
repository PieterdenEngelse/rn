# Secrets and credentials

How rn handles values a job needs but must never write down: where they live,
what protects them, and — the part most documents skip — what does not.

Read `docs/n8n.md` §6 for how this came to be built. This document is the
standing answer to "is my token safe here?", which deserves a plain reply rather
than a reassuring one.

---

## The short version

- A job **declares** the credentials it needs by name and asks for them with
  `ctx.secret("githubToken")`. No token appears in a job file, a request body,
  or a run record.
- Values live in **`~/.config/rn/credentials`**, one `RN_SECRET_<NAME>=value`
  line each. The launcher reads that file and passes the values into the backend
  explicitly.
- **Everything a job reports is scrubbed** of every configured secret before it
  is logged, written to disk, or shown on a page.
- **The file is not encrypted.** Anyone who can read your home directory can
  read your tokens. That is the same protection as an SSH private key with no
  passphrase.
- **Encrypting it is not the most useful next step.** The API has no
  authentication, so any local process can ask rn to *use* a credential without
  reading it. See *What to strengthen first*.

---

## Where values live, and why there

`~/.config/rn/credentials`:

```
# one per line; the name maps to a credential a job declares
RN_SECRET_GITHUB_TOKEN=ghp_...
RN_SECRET_SLACK_WEBHOOK=https://hooks.slack.com/...
```

`chmod 600` it. The launcher warns on startup if it is readable by anyone else,
and reads it anyway — refusing to boot over a permission bit would leave you
with no UI in which to fix anything.

**Not in the install tree, and that is the whole reason this file exists.**
Credentials used to go in `be/.env`, which sits inside the app directory. That
directory is replaced wholesale on upgrade, so every upgrade would have deleted
every token, silently. It is the same failure `settings.json`, the metric history
and the run record were each moved out of the install tree to avoid — applied to
the one kind of value it would hurt most to lose and be least able to reconstruct.

**The name-to-variable rule lives in one place.** `githubToken` becomes
`RN_SECRET_GITHUB_TOKEN`, derived in `be/src/secrets.ts` and nowhere else. The
launcher matches the `RN_SECRET_` prefix and copies a string; it never converts a
name. Two implementations of that rule is how a job ends up reading a variable
nobody set, and failing with an empty header rather than a missing one.

**It does not weaken the environment seal.** `NodeCommand` clears the child's
environment and allowlists what goes in, because a user's `NODE_OPTIONS` can stop
the app booting before its first line runs. Credentials go in through
`NodeCommand::env` — the door for values that come from *us* — not through
`allow_var`. No prefix allowlist, no new trust in the ambient environment.

---

## What a job may do with one

```ts
export const notify: Job = {
    id: "notify",
    credentials: ["githubToken"],
    async run(ctx) {
        const res = await fetch(url, {
            headers: { authorization: `Bearer ${ctx.secret("githubToken")}` },
            signal: ctx.signal,
        });
        ctx.step("called", { status: res.status });
        return { summary: { status: res.status }, changed: false };
    },
};
```

- `ctx.secret` **throws on a name the job did not declare.** That is what keeps
  `credentials` honest: a job quietly reading a credential nobody knows about is
  one the Jobs page cannot warn you about when it goes missing.
- **All declared credentials are required.** The runner refuses to start a job
  whose credential is absent — before the in-flight registry, before the run
  record, before any side effect. An empty `Authorization` header fails somewhere
  far less legible than here.
- **Config → Jobs shows which are set**, by name and variable, never by value —
  and deliberately not by prefix or length either: "starts with `ghp_`" is enough
  to confirm a guess, and a length narrows a search.

---

## Redaction

Every configured secret value is replaced with `[redacted]` in:

| Where | Why it matters |
|---|---|
| `ctx.step` details | Written to the run record *and* to stdout |
| `JobResult.summary` | Rendered on Monitor → Jobs |
| the skip reason | Same |
| the run's recorded input | Nothing stops someone typing a token into a form field |
| an error message | The likeliest place of all — an HTTP client echoing the request URL back into what it threw |
| the error rethrown to the caller | The API puts that message in its 500 body, so it reaches the browser |

Scrubbing is recursive, and covers keys as well as values: a job reporting
`{ [token]: 1 }` has published it just as surely as one reporting
`{ token: value }`.

Two details worth knowing:

- **Values shorter than 8 characters are left alone.** Scrubbing a
  two-character secret would replace those characters inside paths, counts and
  ordinary words, producing a record that looks corrupted rather than protected.
- **Overlapping secrets are replaced longest-first**, or the tail of a long one
  survives when a shorter one nested inside it is replaced first.

Redaction was built before the store, deliberately. A run record is written to
`~/.config/rn/job-runs.json` and rendered on a page, so a job that logs its own
token has published it — and the strongest imaginable store does not undo that.

---

## What this does not protect against

Stated plainly, because a security document that only lists its wins is worse
than none.

- **Anyone who can read your home directory can read your tokens.** The file is
  plaintext. If your machine is shared, or your home directory is backed up
  somewhere you do not control, the credentials go with it.
- **A job that sends a credential somewhere on purpose.** Redaction covers what
  a job *reports*; nothing can cover what it *does*. `View source` is on every
  job row for this reason.
- **A credential that is set is not a credential that works.** Nothing tries it.
  An expired token reads as `set` on Config → Jobs, and the failure appears in
  that job's error log instead.
- **Process memory.** A JavaScript string cannot be overwritten and may be copied
  by the garbage collector, so a credential read into Node is unerasable and
  lands in any heap snapshot or core dump.
- **Anything already leaked.** Redaction only knows values that are configured
  *now*. A token removed from the file stops being scrubbed from records written
  afterwards — and records written earlier keep whatever they kept.

---

## What to strengthen first

The storage question is the one people ask, and it is not the one that decides
how safe the credentials are. This section is the ordering, and the reasoning
for it, so that effort goes where it reduces risk rather than where it feels
like it should.

### The threat model, honestly

| Adversary | `~/.config/rn/credentials` at 0600 | An OS keychain |
|---|---|---|
| Another user on the machine | blocked | blocked |
| **A process running as you** | reads the file | **asks the keyring and is given it** |
| The disk at rest, machine off | plaintext | encrypted under your login password |
| Backups, rsync, cloud sync | swept up as plaintext | an opaque blob |
| macOS specifically | — | per-app ACLs tied to code signing |

The second row is the important one and the least intuitive. On Linux the
freedesktop Secret Service has no per-application isolation: any process in an
unlocked session can request any secret, exactly as rn does. So against the
adversary a keychain is usually imagined to stop, a keychain and a 0600 file are
equivalent.

Where it does win is narrower than its reputation: an encrypted blob is not
swept into a backup or a sync folder as readable text, and it is unreadable
while you are logged out. Full-disk or home-directory encryption dominates the
second of those. macOS is the exception across the whole table — its ACLs are
real, because they are tied to code signing.

### The bigger hole is upstream of storage

**There is no authentication on this API.** `docs/network.md` says so at length
for the remote case; the local consequence belongs here, because it is what
decides the ordering.

Any process running as you can open a connection to the port and
`POST /api/jobs/:id`. It never reads the credential at all — it asks rn to use
it. That is a confused deputy, and while it stands, every storage choice is
equivalent against the adversary the keychain was meant to stop. Strengthening
the vault while the doorman takes instructions from anyone is the wrong order.

The browser half of this *is* closed, and deliberately: the CORS allowlist is
strict, `access-control-allow-credentials` is never set, and a JSON `POST`
preflights — so a page you happen to have open cannot drive the API. CORS is a
browser mechanism and does nothing about `curl`, which is the case above.

### A consequence of how values reach the backend

Credentials are passed to the Node process in its environment, so they are
visible in `/proc/<pid>/environ` to the same user, can land in a core dump, and
would be inherited by anything a job spawns. That is no worse than the file
against a same-user process — it could read either — but a keychain does not fix
it, because the value still ends up in the child's environment. Passing them
over a pipe at startup would close it, independently of where they are stored.

### The order

1. **Scope and expiry of the tokens themselves.** A read-only token that expires
   in a week shrinks every other question on this list, and costs no code.
2. **Authenticate the API.** A per-install token the frontend holds, or — cleaner
   — a unix domain socket at 0600 instead of a TCP port, which makes filesystem
   permissions the authentication and removes the localhost-TCP path outright.
   This is the structural win. See *What rn would need before it could safely
   listen on its own* in `docs/network.md`.
3. **Redaction, and never putting a value on the wire.** Already done. The most
   common way a credential actually leaks is a log line, a screenshot or a bug
   report — not disk forensics.
4. **Full-disk or home-directory encryption.** An operating-system decision, and
   it dominates the at-rest argument above.
5. **An encrypted store.** Buys the backup-sweep row and, on macOS, the ACLs.

**The ordering compounds.** Doing 2 raises the value of 5: once nothing can drive
the deputy, reading the credential becomes the easiest remaining path, and that
is the path a keychain narrows. Doing 5 first buys little while 2 is open.

## Why there is no encrypted store (yet)

The conventional shape is an encrypted file with the key held in the OS keychain.
It is not built, and the reasoning is worth writing down so the decision can be
revisited rather than rediscovered.

- **It is fifth on the list above, not first.** While the API takes instructions
  from any local process, a stronger store does not change what an attacker in
  that position can do.
- **Nothing needs it yet.** No job here authenticates to anything. A store with
  no consumer is the solution-looking-for-a-problem this project's rules reject.
- **It costs three times the launcher's dependency graph.** Measured, not
  guessed: adding `keyring` with `sync-secret-service` and `crypto-rust` — the
  combination that avoids an OpenSSL system dependency and, unlike
  `linux-native`, survives a reboot — takes `cargo tree -p rn` from 18 crates to
  53, mostly D-Bus and crypto. The launcher's own `Cargo.toml` says
  "deliberately tiny: it must run before Node exists".
- **It would not replace the file, only add to it.** The crate cannot enumerate
  entries portably, and the launcher has to know which names to look up before
  it can start the Node process that knows them — so a plaintext index of
  *names* would have to sit beside the store. With the file still needed as the
  fallback for a machine with no Secret Service, the result is three artifacts
  where there is now one.
- **Reaching a keychain from Node means a native addon**, and the runtime rules
  in `CLAUDE.md` prefer a Rust component to an addon. So the real option is a
  small Rust component over the documented CLI boundary.

  It is worth being precise about what that would and would not buy, because
  "Rust can zero a buffer on drop and JavaScript cannot" is true and mostly
  beside the point here: the value still has to cross into Node as a string to
  reach `fetch`, and it is unerasable from that moment. A Rust component would
  protect the value at rest and inside the launcher, not inside the runtime
  where jobs actually use it. The case for it rests on encryption at rest, not
  on memory hygiene.
- **A keychain is a dependency on the machine.** On Linux the usual crates talk
  to the D-Bus Secret Service, which a minimal desktop, a headless box or a
  systemd unit may not have. rn ships its own Node runtime precisely so it does
  not depend on what is installed; a credential store that fails to open on some
  Linux installs would reintroduce that class of dependency one layer down. What
  happens on such a machine needs an answer before the component needs code.
- **Encryption without a decision is theatre.** A key file sitting beside the
  file it decrypts protects nothing while looking like it does — which is worse
  than plaintext honestly labelled, because it stops people taking the other
  precautions they would otherwise take.

**The seam is `read()` in `be/src/secrets.ts`.** An encrypted store slots in
behind it without any job changing, which is why jobs go through it rather than
reading `process.env` themselves.

**The trigger to revisit** is three things together, and the first is the one
that changes the most:

1. The API is authenticated, so a stronger store is the weakest link rather than
   the fifth-weakest.
2. A job actually authenticates to something.
3. There is an answer to "what happens on a Linux box with no Secret Service".

At that point the justification writes itself, and the work is only the store —
reference-by-name, redaction, the declaration on the catalogue and `read()` as
the seam are all in place already, which is what makes deferring it cheap rather
than merely postponed.
