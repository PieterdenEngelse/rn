# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

## 1. There is no way to reset one job's memory

There is no targeted reset at all. `be/src/jobs/state.ts` exports `reset()`,
which is the test seam that clears the in-memory store without saving, and
nothing else removes anything. The supported answer is to delete
`~/.config/rn/job-state.json`, which is all-or-nothing: making one job report
from scratch also makes every other polling job reprocess everything its source
still holds.

Not urgent while there is one polling job. It becomes a real edge the moment
there are two, and the failure is quiet — a person deletes the file to re-run
one report and silently re-triggers another job's whole backlog.

## 2. A permanent rejection is retried as if it were transient

`notify` declares `retry: { attempts: 3, backoffMs: 15_000 }`, which is right
for the failures it was written for — a 502 from a webhook relay, a phone off
wifi. It is wrong for a 400. A receiver that says `invalid_payload` will say it
again in fifteen seconds and again in thirty, and the run takes ninety seconds
to conclude what it knew immediately.

Found while writing the tests, where every failure case sat through the full
sequence. The tests now use a copy of the job with the policy removed and pin
the real policy separately, so the suite is fast — but that is the test working
around the behaviour, not the behaviour being right.

**Why it is not fixed here**: the runner's retry loop is generic and a job has
no way to say "this one will not get better". `UnretryableError` exists inside
`run.ts` for the abort case and is not something a job can throw. The honest
options are to let a job mark an error as permanent, or to let a job supply a
predicate — and neither is worth designing off one case. A second job that
talks to an HTTP API will say which.

The cost while it stands is bounded and visible: ninety seconds, three attempts
on the record, and the parent job held in flight for that long because
`onChange` is awaited.

**A second case, now observed rather than reasoned about.** Verifying the Deno
refusal path (the old item 2) produced exactly this shape from a different
direction: a runtime permission denial is permanent by construction — the grant
is fixed at spawn and cannot change while the process lives — and
`watch-upstreams` still spent **60.2s and three attempts** on it, with two
30-second waits recorded as `retry` steps between three identical
`Requires net access to "nodejs.org:443"` failures. So the "second job that
talks to an HTTP API" this item was waiting on is no longer the only way in:
there are two classes of never-going-to-improve error now, and a permission
denial is the easier of the two to recognise, since `netPermissionHint()`
already has the predicate that identifies it.

## 3. The Deno permission hint is written twice

`netPermissionHint()` exists in both `be/src/jobs/watch-upstreams.ts` and
`be/src/jobs/watch-feeds.ts` — the same regular expression over the same three
Deno error shapes, differing only in which hosts the message names. A third job
that makes an outbound request writes it a third time, and the copy that goes
stale is the one nobody is looking at.

It was left duplicated on purpose until the refusal path had been watched
rather than inferred — sharing a helper nothing has ever executed would make one
unverified thing look like two. **That condition is now met.** Driven live on
deno 2.9.5 against a scratch backend granted only rn's own ports, every lookup
failed with `Requires net access to "nodejs.org:443"` — so of the three arms the
regex tests, that is the one that actually fires, and the advice the hint gives
was checked too: those hosts in `netAllowlist` plus a restart takes the same run
to a clean report. Keep all three arms regardless; that is Deno's wording to
change, and a hint that silently stops firing is worse than one that never did.

So it is foldable now, and the shape is settled: `watch-feeds` takes its hosts
as an argument, which is the general form, while `watch-upstreams` names three
hosts inline. The remaining reason it has not been done is only that it wants
both jobs in one tree, which is after both branches land.

**The cost while it stands**: two regexes to keep in step, and a job author who
copies the wrong one ships a message naming nodejs.org, registry.npmjs.org and
crates.io for a job that talks to none of them.

---

# Decisions waiting on the user

## Twenty junk run records in the real history

`~/.config/rn/job-runs.json` holds twenty records with `jobId: "j"` and
`startedAt` values of 20–39, written by `be/test/settings.test.ts` before it
redirected `config.jobRunsPath`. They render on Monitor → Jobs as twenty rows
reading *"20691d ago · j · unchanged"*.

The leak is fixed and verified by checksumming the real file across a full
`npm test` run. The records themselves are still there. They contain nothing
genuine, but they are the user's data and deleting them is the user's call.

## Nine stale pins, as of 2026-08-26

The first thing `watch-upstreams` produced. Deliberately not maintained as a
list here — the list is what the job is for, and a copy in a document is a copy
that goes stale. Run the job for the current answer.

The snapshot it reported on the day it was written: `typescript ^5.8 → 7.0.2`,
`@types/node ^24 → 26.3.0`, `tailwindcss` and `@tailwindcss/cli
^4.1.14 → 4.3.3`, `daisyui ^5.0 → 5.7.22`, `dioxus` and `dioxus-router
=0.7.9 → 0.7.10`, `gloo-net 0.6 → 0.7`, `gloo-timers 0.3 → 0.4`. Node
`v24.19.0` is current on its line and is the current LTS.

`dioxus` and `gloo-net` are the two with real change behind them. `typescript`
is a major and wants reading about before it is taken.
