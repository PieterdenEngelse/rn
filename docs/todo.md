# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

## 1. `seen()` has no consumer

`ctx.state` is reached in exactly two places in the repo — `get` and `changed`,
both in `watch-upstreams`. The bounded item-id window has thirty-six tests and
no real caller.

That is the situation `docs/n8n.md` rejects in as many words: *"a store with no
consumer is the solution-looking-for-a-problem this project's rules reject."*
The cursor half of the store was shaped by a job that needed it and is right
because of that; the `seen()` half was designed on paper, and nothing has yet
told us whether a thousand ids is the useful bound or whether the
asking-records-it rule survives contact with a source that hands out items in
batches.

**What settles it**: a second real automation that polls *items* rather than
versions — a feed, a mailbox, an issue tracker. The same argument that produced
`watch-upstreams`, one level in.

## 2. Under Deno, `watch-upstreams` is refused and nobody has watched it happen

The launcher grants Deno `--allow-net` for rn's own bind addresses plus the
`netAllowlist` setting, and nothing else — see `launcher/src/layout.rs`. So
`watch-upstreams` is the first thing in rn that outbound network permission
touches at all, and under a Deno runtime every one of its lookups is refused.

`netPermissionHint()` in the job turns that into a message naming the three
hosts to add and where to add them. **It has never been executed.** The
affirmative path was exercised live under Node; the refusal path is inference.

**How to check it**, using the method that works here without disturbing a
running backend: a scratch backend on 399x with its own state files, under the
Deno runtime, and read what the run record says.

## 3. Eleven wire types are still hand-written on both ends

`fe/src/api/wire.rs` still defines eleven structs by hand — the runtime
parameter and config surface: `RuntimeParam`, `ParamsResponse`, `SaveResponse`,
`StatusResponse` and the rest. Every one is a shape `be` also knows and neither
end can be made to agree with the other by the compiler.

This is the drift `shared/` was created to remove: a field renamed in `be`
reaches the browser as `undefined`, at runtime, in whichever panel reads it
first.

`CLAUDE.md` now names these eleven as the remainder and the next to move — it
previously said the crate "covered the jobs surface only", which was the
dangerous half: acting on it meant hand-writing a monitor type that already
existed in `shared/src/monitor.rs`.

## 4. There is no way to reset one job's memory

There is no targeted reset at all. `be/src/jobs/state.ts` exports `reset()`,
which is the test seam that clears the in-memory store without saving, and
nothing else removes anything. The supported answer is to delete
`~/.config/rn/job-state.json`, which is all-or-nothing: making one job report
from scratch also makes every other polling job reprocess everything its source
still holds.

Not urgent while there is one polling job. It becomes a real edge the moment
there are two, and the failure is quiet — a person deletes the file to re-run
one report and silently re-triggers another job's whole backlog.

## 5. A permanent rejection is retried as if it were transient

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

## 6. A screenshot saved under any other name can be committed by accident

`.gitignore:22` ignores `scr-*.png` and nothing else, so a PNG saved as
anything else is one `git add -A` away from being committed. That has already
happened once (`shot.png`, caught by hand).

**Either** widen it to `*.png` at the repository root, **or** agree the `scr-`
prefix is the rule and say so in `CLAUDE.md`'s *Checking the page* section —
where the screenshot command lives, and therefore where anyone about to save
one is looking.

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
