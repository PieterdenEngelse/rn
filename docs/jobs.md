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
