# What to take from n8n

n8n is a node-based workflow automation tool. Most of what makes it popular —
the drag-and-drop canvas — is the part rn should not copy. rn is code-first, and
its answer to "what does this thing do" is `View source` and an info panel. A
visual builder would compete with that rather than serve it.

Its *execution model*, though, is better than rn's in several specific ways, and
each one maps onto a gap already identified in `docs/jobs.md`. This document
lists them in the order worth doing, with what each would actually take.

Nothing here is started. Where it says "currently", that is the state of the
code as written.

---

## 1. Per-step data capture

**What n8n does.** Open any past execution, click any node, and see exactly what
data went in and what came out. This — not the canvas — is what makes debugging
a failed workflow tractable.

**Why it fits rn.** The machinery already exists and is being thrown away.
`JobContext.step()` (`be/src/jobs/types.ts:27`) takes a name and a detail
object, and `runJob` wires it to `log.ts`, which writes one JSON line to stdout.
The launcher inherits stdout rather than capturing it, so from a `.desktop`
launcher those lines go nowhere at all.

Meanwhile `JobRun` (`shared/src/jobs.rs:128`) stores only `summary`, `changed`,
`skipped`, `error`. So the moment a run ends, everything it observed on the way
is gone. `log.ts`'s own docstring says *"every record is raw material for an info
panel in the frontend"* — that is not currently true, because nothing keeps them.

The payoff: the error log stops saying only *"deliberate failure"* and starts
showing the five steps that ran before it and what each one saw.

**Steps.**

1. Add `JobStep { name, at, detail }` to `shared/src/jobs.rs`, and
   `steps: Vec<JobStep>` to `JobRun`. `detail` is a `BTreeMap<String, Value>`,
   the same shape `summary` already uses.
2. Regenerate: `cd be && npm run types:build`.
3. In `runJob` (`be/src/jobs/run.ts`), collect into an array instead of only
   forwarding to `step()` — keep the stdout line, it is still useful under
   `npm run dev`.
4. Pass the collected steps into `record()`.
5. Cap them. A job that calls `ctx.step()` in a loop over 10,000 files would
   otherwise write a 10,000-entry array into a file that is read whole on every
   request. Keep the first N and last N with a marker between, and say in the
   panel that it was truncated — a silent cap is worse than none.
6. Render them in `ErrorLog` and under a run in Recent runs
   (`fe/src/pages/monitor_jobs.rs`), collapsed by default.

**Watch for.** `be/src/jobs/history.ts` writes the whole record file on every
run (`CAPACITY = 200` at line 51). Steps make each entry much larger. Measure
the file after this lands; if it grows past a megabyte or so, steps belong in a
per-run file rather than in the main record.

---

## 2. Error workflows

**What n8n does.** A workflow can name another workflow to run when it fails.

**Why it fits rn.** It is the answer to "nothing pushes" that does not require
building a notification system. Instead of email settings, an SMTP client and a
template, you write a job — and that job can do whatever you want, including
things no built-in notifier would have offered.

It is also the most rn-shaped idea on this list: composable, code-first, and it
makes the failure path a thing you can read the source of.

**Steps.**

1. Add `onFailure?: string` to `Job` (`be/src/jobs/types.ts`), naming another
   job's id.
2. In `runJob`'s catch block, look the id up with `jobById` and run it.
3. Give the handler the failure. Simplest: extend `JobContext` with an optional
   `cause?: JobRun`, set only when the job is running as a failure handler.
4. **Guard against loops.** A job whose `onFailure` points at itself, or at a
   job that fails and points back, must not recurse. Track a depth in the call
   and refuse beyond one hop — and log the refusal rather than swallowing it.
5. Test that the handler runs, that it receives the cause, and that a cycle
   terminates.
6. Show it on the row: "on failure → notify-me", so the wiring is visible
   without reading the source.

**Watch for.** The handler runs through `runJob`, so it is tracked, recorded and
timed like anything else — which is right, but it means a failure produces two
run records. That is correct rather than confusing, provided the page makes the
relationship visible.

---

## 3. Per-job retry with backoff

**What n8n does.** "Retry on fail" is per-node configuration: attempts and wait
time between them.

**Why it fits rn.** rn has no retry at all. `docs/jobs.md` notes the
consequence: for a daily job, a transient failure means waiting a full day.
Network-facing jobs will make this urgent the moment one exists.

**Steps.**

1. Add `retry?: { attempts: number; backoffMs: number }` to `Job`.
2. Implement in `runJob` — it is already the one place everything passes
   through, and doing it anywhere else would let a trigger bypass it.
3. Decide what a retry means for the record, and say so in the panel. One run
   entry with an attempt count is probably right; three entries would make the
   error log read as three separate failures.
4. Interaction with the timeout: is the ceiling per attempt or for the whole
   sequence? Per attempt is the less surprising answer, but it means
   `timeoutMs: 5 min` with three attempts can occupy fifteen. State it.
5. Retries must respect `ctx.signal` — a retry loop that ignores an abort is a
   new way to hang.

---

## 4. Run with pinned input

**What n8n does.** Run a single node with fixed input data, rather than whatever
the previous node would have produced.

**Why it fits rn.** This is the "jobs take no parameters" gap, and n8n's framing
is better than a settings page: the input belongs to the run, not to the
install. "Process this folder" is a thing you say once, not a value you save.

**Steps.**

1. Add an input type per job. Because `shared/` exists, this can be a real
   declared shape rather than a bag of strings — the type lives beside the job's
   other wire types and both ends see it.
2. Accept a JSON body on `POST /api/jobs/:id` (`readJson` already exists in
   `be/src/server.ts`) and hand it to the job through `JobContext`.
3. Validate against the declared shape before running, and return 400 with the
   reason. A job that starts and then fails on bad input has already made its
   first side effect.
4. Render a form on the Jobs page from the declared shape.
5. Record the input in `JobRun`, or the run history becomes unreadable — two
   runs with different inputs would look identical.

**Watch for.** Scheduled runs have no input. Either the type is fully optional,
or a scheduled job declares its defaults; do not let "scheduled" mean "runs with
undefined everywhere".

---

## 5. Filtered execution list

**What n8n does.** Filter executions by status, workflow and time range.

**Why it fits rn.** Recent runs is the last 25, unfiltered
(`be/src/server.ts`, the `/api/jobs` handler). That is fine for one job and
useless for twenty.

**Steps.**

1. Add query parameters to a runs endpoint: job id, outcome, since.
2. Filter server-side. The record is capped at 200 so client-side filtering
   would work today, but it would stop working exactly when it starts to matter.
3. Add the controls to the Recent runs panel.
4. Keep the counts honest: say "12 of 200 retained runs", never a bare "12",
   which reads as a lifetime total. The error log already words it this way.

---

## 6. Credentials as a first-class concept

**What n8n does.** Credentials are stored separately from workflows, encrypted
at rest, and referenced by name so several workflows share one.

**Why it fits rn.** Today there is `be/.env` and nothing else, holding
`LOG_LEVEL` and `DRY_RUN`. The first job that authenticates to anything will
want somewhere better.

This is also the one item on the list with a real argument for Rust. A
JavaScript string cannot be overwritten and may be copied by the garbage
collector, so a credential read from `process.env` is unerasable and lands in
any heap snapshot. A small Rust component holding secrets in a buffer it zeroes
on drop is a component whose one-sentence justification writes itself — which is
the bar `CLAUDE.md` sets for reaching for Rust at all.

**Steps.**

1. Decide the store first: file beside `settings.json`, encrypted with a key
   from the OS keychain, is the conventional shape. Do not invent a scheme.
2. Reference by name from a job — never the value inline, which is how secrets
   reach a log line and a run record.
3. Redact in `ctx.step` details and in `JobRun.summary`. The run record is
   written to disk and rendered on a page; a job that logs its own token has
   published it.
4. Only then consider the Rust component. A credential store in Node with
   correct redaction beats a Rust one without it.

---

## What not to take

**The visual canvas.** rn is code-first and its educational premise is that you
read what runs — `View source` is on every job row. A drag-and-drop graph would
be a second, competing description of the same thing, and the two would drift.

**The expression language** (`{{ $json.field }}`). n8n needs it because its
nodes are configured in a UI and something has to move data between them. rn's
jobs are TypeScript with a real type system and a shared crate behind it.
Swapping that for string templating would be a strict downgrade.

**Sub-workflows and complex orchestration.** Worth revisiting at ten jobs. At
one, it is a solution looking for a problem — and `docs/jobs.md` §3 already
argues that the second front door onto one runner beats a second runner.

---

## Order

1 first: it is the smallest, it uses machinery that already exists, and it makes
every other item easier to debug. Then 2, because it converts a missing feature
into a job you write. Then 3 and 4 when a job actually needs them — which is the
same rule the rest of this project runs on.
