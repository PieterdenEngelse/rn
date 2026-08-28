# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

**Nothing is open right now.** That is a real state rather than a lost file:
the three items that were here — the Deno refusal nobody had watched happen,
the all-or-nothing job memory, and the retry policy that could not be told an
error was permanent — were each closed by doing the thing they asked for. New
items go here, numbered from 1, and they arrive the way every one of those did:
by building something and running into it. The decisions below are a different
list and are still waiting.

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
