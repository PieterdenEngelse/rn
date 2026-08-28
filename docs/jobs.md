# Jobs: what rn can automate, and how one gets started

`be/src/jobs.ts` is a registry with no producers. This document says what the
surrounding code has already decided about the jobs that will fill it, and what
the one remaining structural decision is.

Read `docs/setup-js.md` first for the `be/` layout, and `docs/packaging.md` for
why the runtime is sealed — both constrain what a job may do.

---

## 1. What the design is built for

The categories below are not aspirations. Each is a shape of work that some
existing runtime parameter or module was built to serve, which is a stronger
signal than a roadmap.

### Filesystem-heavy batch work

`threadpoolSize` is a first-class user-facing setting, and its own info panel
says: *"The highest-leverage setting for file automation. A job touching
thousands of files spends its time queued behind these 4 threads — the work is
not slow, it is waiting for a slot."*

This is the best-supported category in the app: the tuning knob shipped before
the job did. It follows from what the libuv pool covers that raising it helps
exactly one shape of workload — many concurrent filesystem, zlib or
password-hashing operations. A job that is mostly network calls, or mostly
computation in JavaScript, is not helped by that number at all.

### Outbound network and API integration

Three settings exist for this and nothing else:

- `netAllowlist` — the hosts a job may reach, *"beyond the app's own listening
  socket"*.
- `extraCaCerts` — TLS to an internal service with a private CA.
- `denoNoRemote` / `bunNoInstall` — closing off fetch-at-runtime, so what ships
  is what runs.

Selecting Deno as the runtime promotes the allowlist from a convention to an
enforced permission: `launcher/src/layout.rs` turns it into
`--allow-net=host,host`, and Deno denies everything not named.

### Long-running work

This is why `be/src/jobs.ts` exists at all. Its opening line is the requirement:

> Exists so a restart cannot silently abort a running automation. Restarting to
> change a memory limit and killing a two-hour job in the process is the kind of
> thing a user never forgives, and never connects to the setting they changed.

It is already wired: `be/src/server.ts` has `/api/restart` consult
`jobs.whenIdle()` and queue behind in-flight work rather than exiting.

### Scheduled and recurring work

Anticipated throughout, built nowhere. Two settings argue their defaults in
terms of a scheduler that does not exist yet:

- `timezone` — *"the time zone every Date and every schedule is interpreted
  in"*.
- `unhandledRejections` — *"Crashing is right for a request handler and arguable
  for an automation driver. One failed job taking the whole scheduler down means
  the other twenty do not run either."*

Twenty jobs on a schedule is the designed target. Assume it when choosing how
jobs are registered.

### CPU- and parse-heavy work

The documented handoff to Rust — hot paths, transform-heavy work, strict data
handling — invoked from Node over a documented interface. See the Node/Rust
split in `CLAUDE.md`; the bar is being able to say in one sentence what the
component does and why Node was the wrong home for it.

### Two constraints that cut across all of them

**`DRY_RUN` defaults to on.** Every job needs a real no-op path that still
reports what it *would* have done — not a flag it accepts and ignores. This is
the difference between a safety switch and a decoration.

**No native addons, and memory ceilings are user-tunable.** `noAddons` exists as
a setting, and `maxOldSpaceSize` is something the user can lower. A job may not
assume native modules or unbounded heap.

---

## 2. The gap: nothing can start a job

The registry is complete and has no callers. This is structural, not a missing
file:

- The launcher runs `be/src/server.ts` — see `launcher/src/layout.rs:207` — not
  `main.ts`.
- `be/src/main.ts`, which `docs/setup-js.md` describes as *"entry point: parse
  args, pick a job, run it"*, parses no args and picks no job. It boots, logs,
  and returns. It is not the process that runs.
- `server.ts` imports `jobs` but only ever **reads** it: `jobs.count()`,
  `jobs.list()`, `jobs.whenIdle()`. Nothing anywhere calls `jobs.track()`.

So the first decision is not *which* job to write. It is how a job gets
triggered.

---

## 3. Why not all three trigger mechanisms?

Three are plausible: an HTTP endpoint on the running server, a scheduler inside
that process, and CLI dispatch through `main.ts`. They are not three peers, and
one of them silently breaks the guarantee §1 just described.

### HTTP and the scheduler are one thing wearing two hats

Both run in the server process: same heap, same tuned `threadpoolSize`, same
`jobs.ts` registry, same restart-deferral. A scheduler is a timer that calls the
same runner the endpoint calls.

Build both. One serves "run it now", the other "run it at 3am", and the second
costs almost nothing once the first exists. There is no tension here.

### CLI dispatch is a different process model, and that is where it bites

A job in a separate process is **invisible to the registry in the server**.
`/api/jobs` returns nothing, Monitor → Jobs reads "Nothing running", and
`/api/restart` will cheerfully restart while that job is mid-run.

That is precisely the failure `jobs.ts` was written to prevent. The protection
would be built, and not covering the path that most looks like it should be
covered.

Three frictions are already in the code, none of them hypothetical:

1. `main.ts:assertSealedEnvironment()` **throws** in an installed tree without
   `RN_ENV_SEALED=1` or `RN_DEV=1`. `npm start` on a user's machine refuses to
   run, by design.
2. `npm start` resolves `node` from PATH, which the Runtime Rules forbid
   outright. Only the launcher-spawned path gets the bundled runtime, the tuned
   V8 flags, and `env_clear()`.
3. Under Deno the grants in `runtime_argv` are shaped for the *server* —
   `--allow-net` is the bind address plus `netAllowlist`, plus blanket
   `--allow-read` and `--allow-write`. A job process inherits exactly that,
   which is both more than most jobs need and possibly less than a specific one
   does.

### What to build

**One runner.** A function that takes a job, wraps it in `jobs.track()`, and
honors `DRY_RUN`. Give it two in-process front doors — HTTP and scheduler. That
is "all three" minus the one that costs something.

**Out-of-process is a fourth thing, added when a job earns it** — one that would
blow the server's heap or monopolize its threadpool. The machinery exists
already: `runtime_argv` takes `entry: &Path`, and its doc comment says it is
*"pure and public"* so that callers beyond the main spawn can use it;
`NodeCommand` seals the environment. What is missing is a cross-process
registry, so a spawned worker registers back and the restart guarantee still
holds.

**The version to avoid** is the tempting one: `npm run job:foo` as a
convenience, unregistered, working fine in development — and quietly unprotected
the first time someone restarts during a real run.

---

## 4. What a job owes the user

Per the Educational by design rules in `CLAUDE.md`, a job is not finished when
it works. Each one carries its info panel in the same change, and the panel says
what the job did rather than that it ran: counts, durations, paths, why it was
skipped, and what the next run will do.

A job whose completion is reported as "done" has failed the product
requirement, however correct its output.

## 5. Where a job's configuration is visible

A job declares its schedule, its timeout and its failure handler in its own
file, and exists at all because it is in the `JOBS` array in
`be/src/jobs/index.ts`. **Config → Jobs** reads those back from the running
backend, beside the settings every run is subject to whatever the job:
`DEFAULT_TIMEOUT_MS` from `run.ts`, `TICK_MS` from `scheduler.ts`, the two
history capacities from `history.ts`, and the `DRY_RUN` switch.

It is deliberately a page of readings rather than inputs. A schedule that lives
in TypeScript shows up in a diff and can be asserted on by a test; the same
schedule in `settings.json` is a value somebody changed at some point, with no
record of who or why. Config → Runtime edits settings because those are
properties of the process; this page reports, because these are properties of
the code.

The numbers reach it over `GET /api/jobs` as `JobsConfig` and `CatalogueJob`,
both defined in `shared/src/jobs.rs` — so a constant renamed in `be/src/jobs/`
breaks a build rather than leaving the page quietly claiming the old value.
That is also why the constants are exported: a page that repeated them would go
on being wrong for as long as nobody checked.

## 6. What a job remembers between runs

A job that polls — an API, a feed, a mailbox, a list of releases — has to answer
one question before it can do anything useful: *have I seen this already?* A
process that starts, runs and exits has no way to answer it. The run history is
a log, not a place to look things up, and a job that keeps its own file beside
the code loses it on the next upgrade.

So there is one store, `be/src/jobs/state.ts`, reached only as `ctx.state`, and
written to `~/.config/rn/job-state.json` beside the settings and the run
history. It is the *State Manager* row of `docs/trigger-archi` — last timestamp,
last hash, last item id — and it exists because `watch-upstreams` needed it,
rather than because the sketch listed it. That order matters: a store designed
before its first consumer guesses at what it must hold, and `docs/n8n.md` makes
the same argument against building the credential store early.

**The rules are the runner's, not the job's.** A cursor is easy to get wrong in
ways nothing reports afterwards, so `runJob` enforces the three that matter:

- **A cursor moves only when the run finishes.** Every write is staged and
  committed after `run()` returns, never after it throws. A job that reads fifty
  items, advances the cursor, and then fails on item three has told the next run
  that all fifty were handled — and the record says only that a run failed.
- **Each retry attempt starts from what is committed.** An attempt that failed
  halfway cannot leak its cursor into the attempt that succeeds.
- **A dry run commits nothing.** `DRY_RUN` means make no change, and a cursor is
  the change that makes the *next* run wrong rather than this one.

Both outcomes are on the record: a run that moved a cursor leaves a
`state-committed` step naming each key and what it moved from and to, and a dry
run leaves `state-withheld` saying why. A cursor that silently did not move is
the explanation for a run that reported nothing, and without the step you are
left comparing timestamps to work out why.

**One consequence worth knowing before arming anything.** For a job that only
reads and reports, the cursor is the *only* thing dry run withholds — so an
unarmed install reports the same items every single run, correctly and forever.
That is not a bug in the switch, but it does mean `DRY_RUN` changes what a
polling job *shows you* rather than only what it touches. `watch-upstreams`
says so in its skip line rather than leaving you to work it out.

The store is deliberately small enough that it cannot become a database:
thirty-two cursor keys per job, four kilobytes per value, and a bounded window
of recently-seen item ids. The failure those caps exist to catch is the key
built from the data — `set(`seen:${item.id}`, true)` works on the first run and
grows without bound after it. `ctx.state.seen(id)` is the supported way to say
that, and it is bounded; `watch-upstreams` avoids the same trap by keeping one
key per *ecosystem* holding a small map, rather than one key per package.

**`seen()` is a window, not a memory**, and the difference is the one thing in
this store that will eventually surprise someone. The oldest id falls off when
the thousand-and-first arrives, and an item whose id has aged out reads as new
again — so it protects against reprocessing what was seen *recently* and does
not promise an item is handled once for all time. A source that emits more than
the window between two runs needs a timestamp cursor instead, because no bounded
set can do that job. It is worth saying because of how it fails: correctly for
months, and then a reprocessed backlog after one outage, at which point nobody
suspects the cap.

**`watch-feeds` is the consumer, and it is where the bound becomes arithmetic.**
A feed cannot use a cursor at all — entries arrive backdated, entries are edited
in place, and newest-first is a convention rather than a contract, so a
timestamp cursor silently skips the post published on Tuesday and syndicated on
Thursday. Per-item identity has none of those failures, which is what `seen()`
is for. What that job had to work out is that the number to watch is not the
capacity but the product: *feeds x entries examined per run* has to stay under
the window, or one run pushes out ids the same run recorded. It checks that
before it does any work and puts a `window-too-small` step on the record, since
the alternative is finding out months later. Two things it added on top of the
store are worth copying: an id is hashed and **qualified by its source**, because
two feeds hand out the same guid all the time and an unqualified id would make
the second copy read as already handled; and the entries below the per-run line
are never asked about, so they stay new to the next run rather than being
consumed by a question nobody acted on.

**Config → Jobs reports it**, on the same board as the run-history capacities:
the two caps, how many cursors are held, and across how many jobs. Counts only,
and there is no endpoint that will return a stored value — a cursor is whatever
the source uses as an identifier, a message id or a URL or an account
reference, and `docs/token-sec.md` is the argument for why reporting that
something is remembered is a different act from showing what.

**One job's memory is cleared from its row on Monitor → Jobs**, and the whole
store by deleting `~/.config/rn/job-state.json`. The targeted one is
`DELETE /api/jobs/:id/state`, which removes that job's cursors and its seen-id
window and reports how many of each it dropped — counts, never values, for the
reason the paragraph above gives. It is refused with a 409 while the job is
running: a run stages its memory and commits when it finishes, so a reset in the
middle would be overwritten seconds later by writes the caller cannot see, and
being told a reset worked when it did not is worse than being told to wait.

It exists because the whole-store version stopped being adequate the moment
there were two polling jobs. Deleting the file to re-run one report also makes
every other polling job reprocess whatever its source still holds — silently,
since an absent cursor is indistinguishable from a first run.

**Either way, the next run is quieter rather than louder**, which is the part
that surprises people. A job whose memory is gone treats its source as unseen —
and both polling jobs answer that by recording where it stands and announcing
nothing, exactly as they do on a fresh install. Reaching for a reset to make a
job "report everything again" gets you a silent run; the jobs' own catch-up
inputs are what report the standing list.

Nothing else notices a cursor is gone, because an absent one is exactly what a
first run looks like — which is also why nothing prunes the entries of a job
that has left the catalogue. Commenting a job out of `JOBS` for an afternoon
should not silently delete the mark that stops it reprocessing its whole source
when it comes back.

## 7. When a failure is not worth retrying

A job declares `retry: { attempts, backoffMs }` and the runner honours it for
every failure — which is right for the ones a retry policy is written for. A
502 from a webhook relay, a registry having a bad minute, a laptop whose wifi
has not woken up: all answered by asking again.

Some failures answer identically every time, and the runner cannot tell which
from the outside. **Throw `PermanentFailure` from `be/src/jobs/permanent.ts`**
and the run fails on the spot:

```ts
if (isPermanentStatus(res.status)) {
    throw new PermanentFailure(message, "the receiver rejected the request itself");
}
throw new Error(message);        // a 502 is what the three attempts are for
```

The second argument is the reason, and it is not decoration: it lands in the
step trace as `retry-skipped`, and it is the answer to *"why did my `retry: 3`
job only run once"* — a question that has to have one, the same way
`retry-abandoned` does.

**It marks one error, not one job.** The policy stays declared and still covers
everything else the same job can hit. `watch-feeds` refuses to retry a
permission denial and still gets all three attempts on a feed that is merely
down.

**Three shapes qualify**, all of them in the tree today:

- **A 4xx from something you POSTed to.** `isPermanentStatus()` is the test, and
  it excludes 408 and 429 — 4xx by numbering, "try again" by meaning.
- **A malformed input or credential.** A format string with a typo in it is
  still a typo on the third attempt.
- **A runtime permission.** Under Deno the network grant is fixed when the
  process starts and cannot widen while it runs, so `netPermissionHint()`
  returning a string *is* the classification — see `be/src/jobs/net-permission.ts`.

**Do not reach for it on a hunch.** The cost of retrying a permanent failure is
bounded and visible — three attempts and two waits, on the record. The cost of
marking a transient failure permanent is a job that gives up on a blip and
reports a failure a retry would have absorbed, and nothing on the page will say
so. When it is not obvious, retry.

Why the job marks it rather than the runner asking, and why not a predicate:
the doc comment at the top of `be/src/jobs/permanent.ts`.
