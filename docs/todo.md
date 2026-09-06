# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

**Nothing is open right now.** The item that stood here — the hooks listener
refusing form-encoded deliveries it had just authenticated, and rejecting
Stripe's and Slack's signature schemes outright — was closed by building
`docs/n8n.md` §7 steps 1 to 3. Steps 4 to 6 there are not open work: each waits
on a provider that needs it, and building one first is how a narrow feature
becomes a default.

The three items that were here before — the Deno refusal nobody had watched
happen, the all-or-nothing job memory, and the retry policy that could not be
told an error was permanent — were each closed by doing the thing they asked
for. New items arrive the way those did: by building something and running into
it. The decisions below are a different list and are still waiting.

---

# Decisions already taken

Nothing on this list is waiting on anyone. It is here so the next person does
not re-decide it, and so a report that keeps naming these does not read as
neglect.

## `effectFree` stays code, though it sits among controls — 2026-09-06

Config → Jobs now edits five fields on every job: schedule, timeout, on
failure, on change, retry. The sixth row on that card, **While disarmed**, is
`effectFree` in the job's own file, and it is deliberately not editable.

Every other overridable field changes *what rn does*. This one changes **what
rn believes about the job's code**. `effectFree: true` is a job declaring that
it writes nothing outside rn — every request a GET — and what rn does with that
declaration is let the job keep its cursors while dry run is on, so its report
stays incremental instead of repeating. Flipping it from a page would not make
a job read-only; it would tell the safety machinery that a job which deletes
files is read-only, and the job would carry on deleting them.

That makes it the one setting on the page whose wrong value is silent by
construction: nothing fails, nothing is refused, and the only symptom is a
disarmed run quietly keeping state it should have discarded.

If it is ever wanted, the shape is a control with the warning in its own panel
rather than in a comment, and the same read-modify-write as the others. It was
not built because "let me tell rn this job is safe" is not a thing a page
should make easy.

## Five stale pins, as of 2026-09-06 — all deliberate

The standing output of `watch-upstreams`. Deliberately not maintained as a list
here — the list is what the job is for, and a copy in a document is a copy that
goes stale. Run the job for the current answer, and turn *report everything
already behind* on when you do: without it a run reports only what moved since
the last one, which on most days is nothing at all.

What it reported on the day this line was written, as resolved versions rather
than manifest ranges:

| upstream | on | latest | why it stays |
|---|---|---|---|
| `@types/node` | 24.13.3 | 26.4.1 | it is `be/.nvmrc` spelled twice — see below |
| `gloo-timers` | 0.3.0 | 0.4.0 | duplicates a crate `dioxus-web` holds — see below |
| `wasm-bindgen` | 0.2.127 | 0.2.128 | `dx` bundles with 0.2.127 — see below |
| `js-sys` | 0.3.104 | 0.3.105 | same upgrade as `wasm-bindgen`, not a separate one |
| `web-sys` | 0.3.104 | 0.3.105 | same upgrade as `wasm-bindgen`, not a separate one |

All five are decisions, so the job will keep reporting five upstreams forever
and that is correct rather than stale. None of them is work nobody got to.

**The wasm-bindgen trio is one upgrade, and it is blocked by `dx` rather than
by anything in this repository.** Taken on 2026-09-06 in c51d9d7 and reverted
the same hour in d09837b, having broken the dev server.

The three the job names cannot move separately: `web-sys 0.3.104` requires
`js-sys = "=0.3.104"`, which requires `wasm-bindgen = "=0.2.127"`. Nor can those
three move alone — `wasm-bindgen-futures`, `-macro`, `-macro-support` and
`-shared` carry the same exact pins, so `cargo update` naming only the three
reports `Locking 0 packages` and changes nothing. All seven move together or
none do.

What stops it is downstream of the lockfile. `dx` bundles the compiled wasm with
a `wasm-bindgen` CLI whose version is fixed when `dx` itself is built, and the
two schema versions must match exactly:

    rust Wasm file schema version: 0.2.128
       this binary schema version: 0.2.127 (a579ee62b)

`dx 0.7.10 (57d6794)` is on 0.2.127, so the lock must be too. **Take it when a
`dx` ships built against 0.2.128**, and check that first rather than the crates
— the crates were never the constraint.

Two things about how this was got wrong, because both are cheap to repeat.
`cargo check --target wasm32-unknown-unknown` passes on the bump and proves
nothing: the schema check happens at bundle time, so only `dx build` sees it.
And `strings` on the `dx` binary shows `wasm-bindgen-cli@` and
`wasm-bindgen/releases/download/`, which was read as "it fetches a CLI matching
the lock". It fetches a CLI; the version is its own. Downloading the 0.2.128 CLI
and running `--version` then confirmed a binary exists, which was never the
question.

`typescript 5.9.3 → 7.0.2` was the third and was taken in 30d54bb. The reason
is worth one line here because it is the argument that decided it: 6.0.3
measured 6.84s against 5.9.3's 6.81s — the same JavaScript compiler — while
7.0.2, the native port, does the same check in 1.47s at half the peak memory.
There was no conservative middle option, only the one we had and the new one.

`daisyui 5.7.20 → 5.7.28` was taken in 306cdba. Worth one line for the method
rather than the outcome: `^5.0.0` already permitted it, so it was `npm update`
plus the `output.css` that `css:build` regenerates, and the whole 583-byte delta
was two additions this app cannot reach — `.checkbox[aria-checked=mixed]` and
`.join`, neither selector appearing anywhere in `fe/`. Established by grepping
for the two selectors, which is why it needed no look at the page. The next
daisyUI bump deserves the same check and not this conclusion.

**`@types/node` is `be/.nvmrc` spelled a second time.** Its majors track Node's:
26.4.1 describes Node 26, and the pin is v24.20.0. Definitions ahead of the
runtime describe APIs the process does not have — `npm run typecheck` passes,
nothing links, nothing warns, and the failure lands at runtime. Being types-only
is what makes it dangerous rather than cheap: there is no build step left to
catch the disagreement. It moves when the runtime moves. The job will report it
for as long as the runtime is not on the newest Node major, because it asks npm
for the `latest` tag and that tag is the newest major by definition — the job
being right about the registry and wrong about this repository.

**`gloo-timers 0.4` was taken on 2026-09-04 and dropped before it landed.** The
whole release note is "MSRV updated to 1.82" — nothing gained — and
`dioxus-web 0.7.10` holds `gloo-timers 0.3` on an enabled path
(`dioxus-web → dioxus → fe`), so taking 0.4 puts two copies in the wasm bundle
where one is shared today. Small, since the crate wraps `setTimeout`, and
invisible, which is the argument for writing it down rather than against. **Take
it when `dioxus-web` moves to 0.4**, at which point it deduplicates instead.

`gloo-net` left a duplicate of exactly the same shape and it was harmless: its
stale `0.6.0` belongs to `dioxus-fullstack`, which is optional and reaches no
enabled build, so `cargo tree -i --target all` finds no path to it. Same
lockfile, opposite answer — only the dependency graph tells them apart.

## The job has been wrong twice, both times found by taking an upgrade

Worth knowing, because the section above tells you to trust its output.

**It read the lowest of a duplicated crate's locked versions** and called it the
direct dependency, on the grounds that `Cargo.lock`'s first entry is ours. The
lock is sorted by name and then version, so the first is the lowest — and it
reported `gloo-net 0.6 → 0.7` the morning `fe/Cargo.toml` was changed to `0.7`.
Fixed in 94b5932: the parser reads the manifest's requirement and picks the
version that satisfies it.

**It compared npm's caret ranges against the newest release**, so `^4.1.14` read
as behind by 4.3.3 while `node_modules` held 4.3.3. Two of five npm entries were
false — `tailwindcss` and `@tailwindcss/cli` are already current — and the other
three overstated the distance, which is the same category of wrong because the
reader budgets from it. Fixed in 250055b: the npm half reads
`package-lock.json`, as the cargo half always read `Cargo.lock`.

Both were invisible for as long as the report was only ever read; neither would
have surfaced from inspection. And the count in that heading was wrong three
times in one morning — eleven, nine, seven, six — every time it was reasoned to
rather than run. **Run the job.**

Node was the other half of that day: `v24.19.0 → v24.20.0`, two of the original
eleven because `be/.nvmrc` answers both the `node:24` and the `node:lts`
question. Taking it is not only the pin — every checkout with a `be/runtime`
needs `scripts/install-node.sh` re-run, or the bundled runtime and `.nvmrc`
disagree and Monitor → Runtime says so. `dioxus`, `dioxus-router` and `gloo-net`
went the same day.
