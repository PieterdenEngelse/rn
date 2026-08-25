# What to take from n8n

n8n is a node-based workflow automation tool. Most of what makes it popular —
the drag-and-drop canvas — is the part rn should not copy. rn is code-first, and
its answer to "what does this thing do" is `View source` and an info panel. A
visual builder would compete with that rather than serve it.

Its *execution model*, though, is better than rn's in several specific ways, and
each one maps onto a gap already identified in `docs/jobs.md`. This document
lists them in the order worth doing, with what each would actually take.

Items 1 and 2 are done. Nothing else here is started. Where it says "currently",
that is the state of the code as written.

---

## 1. Per-step data capture — **done**

**What n8n does.** Open any past execution, click any node, and see exactly what
data went in and what came out. This — not the canvas — is what makes debugging
a failed workflow tractable.

**Why it fitted rn.** The machinery already existed and was being thrown away.
`JobContext.step()` takes a name and a detail object, and `runJob` wired it to
`log.ts`, which writes one JSON line to stdout. The launcher inherits stdout
rather than capturing it, so from a `.desktop` launcher those lines went nowhere
at all.

Meanwhile `JobRun` stored only `summary`, `changed`, `skipped`, `error`. So the
moment a run ended, everything it observed on the way was gone. `log.ts`'s own
docstring says *"every record is raw material for an info panel in the
frontend"* — which was not true, because nothing kept them.

The payoff: the error log stops saying only *"deliberate failure"* and starts
showing the five steps that ran before it and what each one saw.

**What was built.**

`JobStep { name, at, detail }` is in `shared/src/jobs.rs`, and `JobRun` carries
`steps: Vec<JobStep>`. `runJob` collects each `ctx.step()` call onto the run as
well as forwarding it to stdout, and passes the collection to `record()` on both
the success and the failure path — the failure path being the one the feature
exists for.

The cap is `STEP_HEAD`/`STEP_TAIL` in `be/src/jobs/run.ts`, fifty at each end.
It is applied as steps arrive rather than by trimming at the end, so a job
looping over ten thousand files never builds the ten-thousand-entry array the
cap exists to avoid. What falls out of the middle is replaced by one
`steps-truncated` entry carrying a `dropped` count, stamped with the instant of
the earliest omitted step so it sits in chronological order between the two
halves. That entry renders as an ordinary step row, which keeps the record
self-describing rather than making the page know a magic name.

Records written before this landed have no `steps` at all; `history.ts`
normalises them to an empty list on load, so what is served matches the wire
type that says every run has one.

On the page (`fe/src/pages/monitor_jobs.rs`): a run in Recent runs gets a
"12 steps" toggle, collapsed — that list is for scanning. A failure in the error
log gets the same toggle **expanded**, because someone who opened the error log
is already asking what the job had seen when it broke, and hiding the answer
behind another click would defeat the point. Offsets are shown from the first
step (`+0ms`, `+4.2s`) rather than as clock times. The Recent runs panel has an
info button covering all of it, including what a `steps-truncated` row means.

**Measured.** `be/src/jobs/history.ts` writes the whole record file on every run
(`CAPACITY = 200`), so the size question was real. Two hundred runs of a job
reporting six steps each: **183 KiB**. Two hundred runs *all* at the cap, each
step carrying a path and two numbers — the deliberate worst case: **2.6 MiB**.

The typical figure is comfortable and the worst case is over the megabyte
threshold that would move steps into a per-run file. That move was not made,
because no job here steps more than a handful of times and the rest of this
project runs on "when a job actually needs it". The number is recorded so the
decision is a decision rather than an oversight: the first job that steps per
file over a large tree is the signal to split them out, and lowering the cap is
the cheaper half-measure if that day comes sooner than the appetite for it.

---

## 2. Error workflows — **done**

**What n8n does.** A workflow can name another workflow to run when it fails.

**Why it fitted rn.** It is the answer to "nothing pushes" that does not require
building a notification system. Instead of email settings, an SMTP client and a
template, you write a job — and that job can do whatever you want, including
things no built-in notifier would have offered.

It is also the most rn-shaped idea on this list: composable, code-first, and it
makes the failure path a thing you can read the source of.

**What was built.**

`Job.onFailure?: string` names another job's id. `runJob`'s catch block records
the failure, then looks the id up with `jobById` and runs the handler through
`runJob` — so the handler is tracked, timed and recorded like anything else, and
its source is readable from its own row.

`JobContext.cause?: JobRun` carries the *whole* failed run to the handler: the
error, the duration, and the steps from item 1 that ran before it broke. A
handler told only "something failed" could not say which job.

**The loop guard is the cause, not a counter.** `runJob` takes `cause` as an
internal third argument, and its presence is what marks a run as a handler run —
a handler never starts a handler of its own. A job that names itself, or a pair
that name each other, therefore stops after one extra run. A depth number would
express the same rule in a value nobody can see; this way the guard and the
payload are the same fact, and "handler runs cannot cascade" is true by
construction rather than by arithmetic.

**Three silent failures were closed deliberately**, because each is invisible by
nature:

- An `onFailure` naming an id that does not exist logs `on-failure-missing`.
  Nothing else would ever report it — the job it names cannot fail — so the user
  would go on believing something was watching.
- A refused second hop logs `on-failure-refused` with the reason.
- A handler that throws is recorded as its own failed run, but is **not** allowed
  to propagate: the answer to "why did my job fail" must not be replaced by a
  message about a different job.

**Two run records, made legible.** `Trigger` gained a `failure` variant and
`JobRun` a `causedBy` holding the failed job's id, so the second record says
`on failure of prune-profiles` rather than looking like an unexplained run that
started at the same moment. `CatalogueJob.onFailure` puts `on failure →
notify-me` on the job's row, with an info button beside it covering the
mechanism, the one-hop rule and the three silent failures above — same argument
as the schedule: a failure path nobody can see is indistinguishable from no
failure path at all.

Tests cover the handler running, receiving the cause, both cycle shapes
terminating, both silent-failure logs, and a broken handler leaving the original
error intact.

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

1 and 2 are done. 1 was the smallest, it used machinery that already existed,
and it makes every other item easier to debug — its steps are what a failure
handler now receives. 2 converted a missing feature into a job you write. 3 and
4 wait until a job actually needs them, which is the same rule the rest of this
project runs on.
