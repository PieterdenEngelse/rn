# Node runtime parameters exposed to users

**Generated from `be/src/runtime-params.ts` by `npm run params:build`. Do not edit.**

Node v24.20.0 has 1,038 command-line flags (180 Node, 858 V8) and 19 environment
variables. Almost none of them belong in front of a user. These are the ones that do:
settings someone running rn could plausibly need, understand, and not silently break.

Values were measured on a 4-thread, 7 GB machine — the heap default in particular
scales with installed RAM, so expect a different number elsewhere.

| Setting | Set via | Default | Range | Takes effect |
|---|---|---|---|---|
| **JavaScript runtime** | `jsRuntime` (launcher) | node | — | on restart |
| **Extra network hosts** | `netAllowlist` (launcher) | unset (system default) | — | on restart |
| **Node version line** | `nodeVersion` (launcher) | unset (system default) | — | on restart |
| **Heap memory limit** | `--max-old-space-size` (NODE_OPTIONS) | unset (system default) | 64 … 32768 MB | on restart |
| **libuv thread pool** | `UV_THREADPOOL_SIZE` | 4 | 1 … 1024 | on restart |
| **New-space size (advanced)** | `--max-semi-space-size` (NODE_OPTIONS) | unset (system default) | 1 … 1024 MB | on restart |
| **Bun low-memory mode** | `--smol` (runtime flag) | off | — | on restart |
| **Bun kill orphans** | `--no-orphans` (runtime flag) | off | — | on restart |
| **Bun no auto-install** | `--no-install` (runtime flag) | off | — | on restart |
| **Deno V8 flags** | `--v8-flags` (runtime flag) | unset (system default) | — | on restart |
| **Block native addons** | `--no-addons` (NODE_OPTIONS) | off | — | on restart |
| **Deno no remote modules** | `--no-remote` (runtime flag) | off | — | on restart |
| **Unhandled rejection policy** | `--unhandled-rejections` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Time zone** | `TZ` | unset (system default) | — | on restart |
| **Extra CA certificates** | `NODE_EXTRA_CA_CERTS` | unset (system default) | — | on restart |
| **Mail account** | `RN_MAIL_USER` | unset (system default) | — | on restart |
| **Accept mail only from** | `RN_MAIL_ALLOWED_SENDERS` | unset (system default) | — | on restart |
| **SMTP host** | `RN_SMTP_HOST` | smtp.gmail.com | — | on restart |
| **SMTP port** | `RN_SMTP_PORT` | 465 | 1 … 65535 | on restart |
| **IMAP host** | `RN_IMAP_HOST` | imap.gmail.com | — | on restart |
| **IMAP port** | `RN_IMAP_PORT` | 993 | 1 … 65535 | on restart |
| **Trace warnings** | `--trace-warnings` (NODE_OPTIONS) | off | — | on restart |
| **Trace deprecations** | `--trace-deprecation` (NODE_OPTIONS) | off | — | on restart |
| **Stack trace depth** | `--stack-trace-limit` (NODE_OPTIONS) | 10 | 0 … 200 | immediately |
| **Scheduler tick** | `schedulerTickMs` (settings.json) | 30000 ms | 1000 … 300000 ms | immediately |
| **Dry run** | `dryRun` (settings.json) | on | — | immediately |
| **Log level** | `logLevel` (settings.json) | info | — | immediately |
| **Default job timeout** | `defaultTimeoutMs` (settings.json) | 1800000 ms | 1000 … 86400000 ms | immediately |
| **Cursors per job** | `stateCursorsPerJob` (settings.json) | 32 cursors | 4 … 512 cursors | immediately |
| **Remembered ids per job** | `stateSeenPerJob` (settings.json) | 1000 ids | 50 … 20000 ids | immediately |
| **Runs kept** | `historyCapacity` (settings.json) | 200 runs | 10 … 5000 runs | immediately |
| **Failures kept** | `failureCapacity` (settings.json) | 50 failures | 5 … 1000 failures | immediately |
| **Heap snapshot signal** | `--heapsnapshot-signal` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Disable colored output** | `NO_COLOR` | off | — | on restart |

## runtime

### JavaScript runtime — `runtime`

**What it does.** Which runtime the launcher spawns. Only a bundled runtime is ever used: this never reaches for one on the user's PATH. Select an option to see what it is good at.

**Why you would change it.** The three differ in what they are good at rather than in quality — ecosystem reach, process startup, and enforced permissions. The panel for each option makes the specific case.

**If it's wrong.** Selecting a runtime this install does not carry saves the intent but cannot be honoured: the launcher reports it as unavailable at next start and stays on the bundled runtime, rather than failing to boot and leaving no UI in which to change it back.

Default: node · Takes effect: on restart · Settings key: `jsRuntime`

### Extra network hosts — `netAllowlist`

**What it does.** Hosts the automation is allowed to reach, beyond the app's own listening socket, as a comma-separated list — "api.example.com, 10.0.0.5:5432". The launcher always grants the bind address itself, so this is only for job code that calls outward.

**Why you would change it.** It is only enforced under Deno, and it is the whole reason to run there: the grant is checked by the runtime, so a dependency that quietly phones home is stopped rather than trusted. Under Node and Bun the value is recorded but nothing enforces it — neither has a network permission model.

**If it's wrong.** Too narrow and the job fails with a Deno permission error naming the exact host it wanted, which tells you what to add. Too wide and you have given back the guarantee you switched runtimes for.

Default: unset (system default) · Takes effect: on restart · Settings key: `netAllowlist`

### Node version line — `nodeVersion`

**What it does.** Which Node major line the app should run on. Unset means 'use whatever is bundled', which is the value in be/.nvmrc — currently v24.20.0.

Two entries in this list look like the same thing, and today they are: 'Bundled runtime — no version pinned' and 'Node 24 — current (bundled)' both start v24.20.0. What separates them is the next upgrade. Unpinned follows whatever ships, so an install that moves to Node 26 takes you with it and there is nothing here to change. Pinning 24 says stay on 24 whatever happens — after that upgrade the setting names a line the install no longer carries, the launcher falls back to the bundled runtime and says so, and what you are left with is a setting that disagrees with the process. Leave it unpinned unless you have a reason to freeze the version.

**Why you would change it.** Pin an older line when a native addon has not been rebuilt for a newer one yet. It buys compatibility at the cost of being on a clock: a maintenance line stops getting fixes before the others do.

**If it's wrong.** The mismatch that actually bites is drift — settings asking for one line while the bundled binary is another. The active row above reports what the process really is, so trust that over this and treat any disagreement as the bug.

Default: unset (system default) · Takes effect: on restart · Settings key: `nodeVersion`

## Memory

### Heap memory limit — `--max-old-space-size`

**What it does.** Caps V8's old-space heap. Unset, V8 derives a limit from installed RAM rather than leaving the heap unbounded — the field's placeholder shows what that came out as here, and Monitor → Runtime reports it beside what is actually in use.

**Why you would change it.** Raise it when a large job dies with 'JavaScript heap out of memory'. Lower it to stop rn competing for memory on a shared machine.

**If it's wrong.** Too low and the job dies part-way through. Note it sets old space, not the total: setting 256 produced a 2240 MB → 448 MB total limit, not 256.

Default: unset (system default) · Takes effect: on restart · Settings key: `maxOldSpaceSize`

### New-space size (advanced) — `--max-semi-space-size`

**What it does.** Sizes new_space, the region every object is born into. V8 keeps two halves of it and collects by copying whatever is still alive from one to the other — cheap, because the cost is proportional to what survives rather than to what was allocated. An object that survives a couple of those passes is promoted to old_space, where collection is much more expensive.

So this does not set a memory limit. It sets how long an object gets to prove it is short-lived before being treated as long-lived.

**Why you would change it.** Marked advanced because the direction of the effect is not obvious and depends on the workload. A larger new_space gives objects more chances to die young, which keeps them out of old_space and away from the expensive collector. A smaller one promotes sooner, which means fewer scavenges but more work for the collector that matters.

Neither is right in general. It is worth touching only when the Collection board shows meaningful time spent collecting and the ordinary answers — allocating less, holding less — are exhausted.

**If it's wrong.** Measure it rather than reason about it, and measure the thing you care about. Collection count is a trap: shrinking new_space can lower it simply because objects are promoted out instead of being scavenged repeatedly, which looks like an improvement while making the expensive collector's job harder.

Time spent collecting, on the Collection board, is the figure to watch, read against uptime. If a change does not move it on your own workload, put it back to unset.

Default: unset (system default) · Takes effect: on restart · Settings key: `maxSemiSpaceSize`

### Bun low-memory mode — `--smol`

**What it does.** Runs Bun in a reduced-memory configuration: smaller heap targets and more eager garbage collection.

It is a pressure dial, not a ceiling. Node's heap memory limit sets a hard cap that a job dies against; this only makes Bun try harder to stay small. Bun has no clean equivalent of --max-old-space-size — that is a V8 flag and Bun runs JavaScriptCore — so if you need a guaranteed upper bound rather than a tendency, this is not it.

**Why you would change it.** Worth it when the app shares a machine and the automation is not memory-hungry. Collecting more often trades a little throughput for a meaningfully smaller resident footprint.

**If it's wrong.** On an allocation-heavy job the extra collection shows up as slower wall-clock time for the same work. It is a trade between footprint and speed, not a fix for running out of memory — a job that genuinely needs the memory will still need it, and will still get it.

Default: off · Takes effect: on restart · Settings key: `bunSmol`

### Deno V8 flags — `--v8-flags`

**What it does.** Passes flags straight through to V8, comma-separated — for example --max-old-space-size=512,--max-semi-space-size=64. Deno runs V8 like Node does, but does not read NODE_OPTIONS, so this is the only route to V8 tuning under it.

**Why you would change it.** For V8 flags this app does not model. The common one no longer needs it: the Heap memory limit above works under Deno now, because the launcher folds it into this same --v8-flags argument rather than leaving it in NODE_OPTIONS, which Deno ignores.

Anything set here is merged with what the launcher adds, into a single --v8-flags — passing two would silently keep only the last.

**If it's wrong.** V8 rejects an unknown flag at startup, so a typo means the process does not come up rather than quietly running unconfigured. Check the launcher output if it fails to start after a change here.

Default: unset (system default) · Takes effect: on restart · Settings key: `denoV8Flags`

## Concurrency

### libuv thread pool — `UV_THREADPOOL_SIZE`

**What it does.** Size of libuv's thread pool: a fixed set of operating-system threads that exist to do blocking work off the main thread. Default 4, regardless of how many cores the machine has.

These threads never run JavaScript. A thread takes a blocking call, waits in the kernel for it to finish, and posts the result back to the event loop, which then runs your callback on the one JavaScript thread as usual. The pool is how a single-threaded runtime does slow I/O without stopping.

It covers filesystem calls, dns.lookup, zlib, and the crypto functions with no non-blocking form — pbkdf2, scrypt, randomBytes. It does not cover network sockets: those are event-driven through epoll or kqueue and need no thread at all, which is why a server handling thousands of connections is unaffected by this number. Note dns.lookup uses the pool but dns.resolve does not, because the first calls the blocking system resolver and the second speaks DNS over a socket.

This is not node:worker_threads, despite the similar names. Those are real JavaScript threads you create in code, each with its own V8 isolate and its own event loop, and this setting has no effect on them. The threads here are invisible: you never see one, never schedule onto one, and only notice them as the reason a filesystem job is or is not waiting.

**Why you would change it.** The highest-leverage setting for file automation. A job touching thousands of files spends its time queued behind these 4 threads — the work is not slow, it is waiting for a slot.

It follows from what the pool covers that raising it helps exactly one shape of workload: many concurrent filesystem, zlib or password-hashing operations. If the automation is mostly network calls, or mostly computation in JavaScript, this number changes nothing.

One more thing decides whether it bites at all, and it is not a setting: the libuv version, shown on Monitor → Runtime. In libuv 1.45.0 file reads, writes, fsync, fdatasync and the stat calls moved to io_uring on Linux, bypassing this pool entirely; 1.49.0 reverted that, and they run on the pool again unless the loop opts in. The runtime bundled here carries 1.52.1, so file work is on the pool and this setting is the lever. On a runtime carrying 1.45 to 1.48 the same change would barely move a read-heavy job, because the kernel would be doing the reads.

**If it's wrong.** Too high wastes memory and adds contention. Above 1024 libuv silently clamps — a value of 2000 starts with no warning and behaves as 1024.

The symptom of it being too low is a job that is slow while the machine looks idle: low CPU, low event-loop utilisation, and the Monitor's thread pool figure sitting at its ceiling. That combination is this setting and almost nothing else.

Changing it while the app runs does nothing at all. libuv builds the pool on the first operation that needs one and never resizes it, which is why this is a restart setting rather than an immediate one — the constraint is libuv's, not the page's.

Default: 4 · Takes effect: on restart · Settings key: `threadpoolSize`

### Bun kill orphans — `--no-orphans`

**What it does.** Makes Bun exit when its parent process dies, and kill every descendant of its own on the way out. Without it a child outlives whatever started it and keeps running unattached.

**Why you would change it.** It matches how rn is meant to run. The launcher supervises the backend, so a backend still alive after the launcher is gone is not doing anyone any good — it holds the API port and the next launcher cannot bind it. Orphaned processes are hard to notice precisely because nothing is watching them.

**If it's wrong.** The hazard is the opposite of the one it fixes: work you deliberately detached dies with the parent too. If a job spawns something meant to outlive the run, this kills it.

Default: off · Takes effect: on restart · Settings key: `bunNoOrphans`

## Network

### Bun no auto-install — `--no-install`

**What it does.** Turns off Bun's auto-install. By default Bun fetches a missing package from the network mid-run rather than failing on the import, which is convenient in a scratch script and surprising in a shipped app.

**Why you would change it.** An installed app that reaches the network unasked is the thing the sealed environment exists to prevent. It also makes runs deterministic: what is on disk is what executes, rather than whatever the registry served that afternoon.

**If it's wrong.** A genuinely missing dependency now stops the run with a resolution error instead of quietly appearing. That is the point — the error names the package, and installing it deliberately is a decision rather than a side effect.

Default: off · Takes effect: on restart · Settings key: `bunNoInstall`

### Deno no remote modules — `--no-remote`

**What it does.** Refuses to resolve a module from a URL. Deno imports can name a remote address directly, and by default it will fetch and cache one at first run; this makes that an error instead.

**Why you would change it.** It is the Deno half of what Bun's no auto-install does, and sharper: an import specifier is a URL, so code can reach the network simply by existing. A shipped app should execute what is on disk and nothing it downloaded on the way. --cached-only is the softer version, allowing a remote module only if it is already cached.

**If it's wrong.** An import naming a URL now fails at resolution, before anything runs. The error names the specifier, which makes vendoring it a deliberate step rather than something that already happened.

Default: off · Takes effect: on restart · Settings key: `denoNoRemote`

### Extra CA certificates — `NODE_EXTRA_CA_CERTS`

**What it does.** Path to a PEM file of additional trusted certificate authorities, added to Node's built-in list.

**Why you would change it.** Needed behind a corporate proxy that re-signs TLS traffic — the classic 'works at home, fails at the office' failure.

**If it's wrong.** A wrong path fails SILENTLY: Node starts with no warning and TLS keeps failing. rn validates the path itself and reports it, because Node won't.

Default: unset (system default) · Takes effect: on restart · Settings key: `extraCaCerts`

## security

### Block native addons — `--no-addons`

**What it does.** Makes process.dlopen throw instead of loading a native addon, and turns off the "node-addons" export condition so a package resolving a native build for itself gets the JavaScript one instead. A blocked call fails with ERR_DLOPEN_DISABLED, which names the cause rather than looking like a missing file.

**Why you would change it.** A native addon is compiled C++ running inside this process with none of the language's guarantees: it can corrupt memory, crash the runtime outright, and it has to be rebuilt per platform and per runtime version. The project's own rule is to prefer moving that work to a Rust component invoked over a documented interface. This is that rule enforced rather than trusted — a dependency cannot quietly pull one in.

**If it's wrong.** A dependency that genuinely needs an addon stops working, loudly and at the point of loading. That is the intended outcome: the error names the package, and the decision of whether it belongs here becomes explicit.

Default: off · Takes effect: on restart · Settings key: `noAddons`

### Dry run — `dryRun`

**What it does.** The safety switch, on by default. Every job is handed it as ctx.dryRun and honours it by doing all of its work except the part that writes — the scan, the comparison and the decision all still happen, so what it reports is what an armed run would actually do.

It is the whole process, not per job: there is no override, which is what makes "is anything armed right now" a question with one answer. Read when each job starts, so changing it applies to the next run and never to one already going under the value it began with.

One thing it no longer withholds from every job: a job that declares it changes nothing outside rn — every request a GET — keeps the cursor recording what it saw, so its report stays incremental while this is on. Config → Jobs names those jobs on the "While disarmed" row. Nothing else changes: such a run still reports changed: false, so it hands off to no follow-up job.

**Why you would change it.** Because this is an automation tool, and the failure mode of a mistake is not a crash — it is something irreversible happening to your files or to someone else's service. On means a misconfigured job produces a report instead of damage, and you arm it once you have read that report.

Until now the only way to change it was editing DRY_RUN in be/.env and restarting, which is the worst affordance in the app attached to its most consequential switch. Note the inverted check there: anything other than the exact string "false" means dry run, so a typo fails safe.

**If it's wrong.** Left on, every job reports what it would have done and nothing ever happens — which looks exactly like a broken automation if you are not expecting it. Monitor → Jobs shows a banner while it is on for that reason.

Turned off before you have read a dry run, the first thing you learn about a bad filter is what it deleted. Arming is written to the log at warn level in both directions, because it is the one change that must outlive whoever made it forgetting.

Default: on · Takes effect: immediately · Settings key: `dryRun`

## Diagnostics

### Unhandled rejection policy — `--unhandled-rejections`

**What it does.** What happens when a promise rejects and nothing is there to catch it. The default is to crash: an unhandled rejection is treated as an uncaught exception and the process exits.

**Why you would change it.** Crashing is right for a request handler and arguable for an automation driver. One failed job taking the whole scheduler down means the other twenty do not run either. warn-with-error-code is the middle position — the run continues, every rejection is logged, and the exit status still says something went wrong, so a supervisor or a CI step notices.

**If it's wrong.** warn and none turn a crash into a silence, and silence is how a job that half-finished starts looking like a job that succeeded. Only reach for them if something else is checking the work actually happened.

Default: unset (system default) · Takes effect: on restart · Settings key: `unhandledRejections`

### Trace warnings — `--trace-warnings`

**What it does.** Prints a full stack trace with every process warning.

**Why you would change it.** A bare warning tells you something happened but not where. Turn this on when you need to find the origin.

**If it's wrong.** Noisier logs. Nothing breaks.

Default: off · Takes effect: on restart · Settings key: `traceWarnings`

### Trace deprecations — `--trace-deprecation`

**What it does.** Prints a stack trace when a deprecated API is used.

**Why you would change it.** Shows what will break before a Node upgrade, and which code to fix.

**If it's wrong.** Noisier logs. Nothing breaks.

Default: off · Takes effect: on restart · Settings key: `traceDeprecation`

### Stack trace depth — `--stack-trace-limit`

**What it does.** How many frames an Error captures. The one setting here that takes effect immediately — it maps to Error.stackTraceLimit and needs no restart.

**Why you would change it.** Raise it when a truncated trace hides the actual cause of a failure.

**If it's wrong.** Large values slow down code that throws frequently, since every Error captures more frames.

Default: 10 · Takes effect: immediately · Settings key: `stackTraceLimit`

### Log level — `logLevel`

**What it does.** How much the backend writes to its own output. It filters stdout and nothing else — in particular it does not touch what a run remembers. A job's progress goes to two places, the run record and stdout, and only the second is filtered here. Turning this down makes the terminal quieter and changes the Jobs page not at all.

info is what the app did: jobs starting and finishing, settings saved, the scheduler firing. debug adds one line per HTTP request, which on a page that polls is most of the output. warn and error keep only the lines you would act on.

**Why you would change it.** Mainly for reading the log by hand. Under npm run dev the frontend polls several endpoints a second, so debug is unreadable and info is what you actually want; when you are watching for one specific failure, warn cuts everything else away.

It takes effect on the next line, with no restart, and a change announces itself at error — so a log going quiet is never ambiguous between "filtered" and "stopped".

**If it's wrong.** Set to error and you lose the record of ordinary work: a job that ran and changed nothing writes nothing, which reads as an automation that never fired. The Jobs page still has every run, so check there before concluding anything is broken.

Set to debug on a machine that has been running a while and the useful lines are buried under request chatter.

Default: info · Takes effect: immediately · Settings key: `logLevel`

### Cursors per job — `stateCursorsPerJob`

**What it does.** How many named marks one job may keep in ~/.config/rn/job-state.json. A cursor is how a job remembers where it got to — the id of the last item it handled, a timestamp, a hash — so the next run can start from there instead of from the beginning.

The cap is checked when a job writes a key it has not used before. Raising it applies at once; lowering it never deletes marks a job already wrote, because a cursor removed behind a job's back is that job reprocessing everything it had already handled. What a lower number does is refuse the next new key, and the job is told so as an error.

**Why you would change it.** It is the bound that keeps a memory a memory. The failure it is aimed at is a key built from data — set(`seen:${item.id}`, true) looks reasonable, works on the first run, and turns the state file into an unbounded log of every item that has ever arrived. seen() is the supported way to say that and is bounded separately.

Raise it for a job that legitimately tracks many sources — one mark per feed, per repository, per queue — and hits the ceiling for a good reason rather than a careless one.

**If it's wrong.** Too low and a job stops being able to record where it got to. The run does not fail quietly: set() throws and the run is recorded as a failure naming the key it could not add, which is the one moment anybody is looking.

Too high and the guard stops guarding. The state file is read whole at startup and written after every run, so a job accumulating a key per item makes every later run slower, and nothing reports it until the file is large enough to notice.

Default: 32 cursors · Takes effect: immediately · Settings key: `stateCursorsPerJob`

### Remembered ids per job — `stateSeenPerJob`

**What it does.** How many recently-seen item ids one job keeps, so seen() can answer whether an item has been handled before. Newest last; the oldest falls off when the next one arrives.

This is a window, not a memory: an id that has aged out reads as new again. Lowering it trims what is already held on the next commit rather than describing a future the file does not match — which does mean the trimmed ids are new again, and a job can re-report entries it had already seen.

**Why you would change it.** It decides whether a job's dedupe survives its own run. The arithmetic is small: watch-feeds examines feeds x entries ids in one pass, so three feeds at twenty is sixty, and a window of a thousand holds months of steady state because only genuinely new entries consume any of it.

Raise it when a job handles more items per run than the window holds — at that point a single run pushes out ids it recorded itself, and entries start being reported twice. The runner warns when it sees that happen.

**If it's wrong.** Too small and items are announced again after they age out — the classic shape is an automation that reports the same three releases every morning, with nothing to say the window is the cause.

Too large and the state file grows: it is read whole at startup and rewritten after every run, so twenty thousand ids per job across several jobs is a file every run pays for.

Default: 1000 ids · Takes effect: immediately · Settings key: `stateSeenPerJob`

### Runs kept — `historyCapacity`

**What it does.** How many run records are kept before the oldest falls off. They live in ~/.config/rn/job-runs.json, which is read whole at startup and rewritten after every run.

Lowering it takes effect at once and trims what is already held, rather than describing a future the file does not yet match.

**Why you would change it.** It decides how far back you can answer "has this been failing all week, or only today?". A job on a fifteen-minute schedule fills 200 records in about two days, so anyone running more than a couple of automations outgrows the default quickly — and the evidence is gone before they think to look for it.

The ceiling exists because the file is rewritten on every run. Unbounded history is how a JSON file becomes a performance problem nobody notices until it is one.

**If it's wrong.** Too small and the run you want has already been evicted. Nothing announces the loss — the list simply starts later than you expected, which reads as "it never ran" rather than "it was forgotten".

Too large and startup slows and every run pays to rewrite a bigger file. Failures are kept in their own list, so raising this is not how you keep failures longer.

Default: 200 runs · Takes effect: immediately · Settings key: `historyCapacity`

### Failures kept — `failureCapacity`

**What it does.** How many failed runs are kept, in a list of their own, separate from the run history above.

**Why you would change it.** Because a single bounded list gets this exactly backwards. Failures are the rare, valuable entries, and they are precisely the ones a run of successes evicts: a job that failed twice in March and has succeeded nightly since would have no trace of March left — which is the history someone opens an error log to read.

So failures are kept separately, and for longer in effective terms. Fifty failures is a lot of failures; if a job has more than that, the oldest are not the ones you need.

**If it's wrong.** Too small and an intermittent fault older than the last few failures is invisible, which is the fault most worth seeing. Too large and the same rewrite cost as the run list, for records that are rarer.

Default: 50 failures · Takes effect: immediately · Settings key: `failureCapacity`

### Heap snapshot signal — `--heapsnapshot-signal`

**What it does.** Writes a V8 heap snapshot when the process receives this signal, e.g. SIGUSR2.

**Why you would change it.** Captures memory state from a running job to diagnose a leak.

**If it's wrong.** Snapshots are large and pause the process while written. Choosing a signal the process uses for something else can kill it.

Default: unset (system default) · Takes effect: on restart · Settings key: `heapSnapshotSignal`

## Time

### Time zone — `TZ`

**What it does.** The time zone every Date and every schedule is interpreted in. Left unset, Node follows the operating system, and the placeholder shows which zone that currently resolves to. The dropdown lists the common zones; any other IANA name can be typed in.

**Why you would change it.** Pin it when jobs must run at a fixed local time regardless of what the machine thinks, or when logs are compared across machines. Use an IANA name such as Europe/Amsterdam or UTC.

**If it's wrong.** Nothing errors. Timestamps are quietly wrong and scheduled jobs fire at the wrong hour — usually noticed only after a daylight-saving change.

Default: unset (system default) · Takes effect: on restart · Settings key: `timezone`

### Scheduler tick — `schedulerTickMs`

**What it does.** How often the scheduler wakes and asks whether any job is due. It is not how often jobs run — a job scheduled daily at 03:00 still runs once a day. This is only the resolution with which "03:00" is noticed, so a run can start up to one tick late.

Polling a clock rather than setting a timer per job is deliberate: a long timer is wrong across a laptop suspend, where the machine sleeps at 22:00 and wakes at 09:00. Asking "is anything due?" every half minute comes out right whether it slept or not.

**Why you would change it.** It is the worst case for how late a scheduled run can be, and the number to reach for when a schedule looks like it is drifting. Lower it when you have a job on a short interval and the lateness matters; raise it on a laptop where waking twice a minute to find nothing due is battery spent for nothing.

Changing it takes effect immediately and does not reschedule anything — every job's next run stays at the moment it was already due.

**If it's wrong.** A schedule cannot be finer than the interval that checks it. Set this to five minutes and a job asking to run every two minutes fires every five instead — quietly, because nothing has failed. Config → Jobs shows the cadence beside each job's schedule so the two can be read together.

Very low values do not break anything; they just spend wakeups. The scheduler does no work on a tick when nothing is due.

Default: 30000 ms · Takes effect: immediately · Settings key: `schedulerTickMs`

### Default job timeout — `defaultTimeoutMs`

**What it does.** The wall-clock ceiling applied to a job that does not name its own. When it passes, the run's AbortSignal fires, the run is recorded as failed with the elapsed time, and the runner stops waiting.

"Stops waiting" is the exact wording. A JavaScript promise cannot be killed from outside, so the timeout ends the runner's interest in the job, not the job itself — work that ignores ctx.signal carries on holding whatever it holds until the process restarts.

Read when each run starts, so a change applies to the next job that begins and never to one already counting down.

**Why you would change it.** It is what stops one wedged job from becoming a wedged install. A job that hangs on a socket would otherwise sit in the in-flight list forever, and the backend waits for running jobs before a restart — so a single hung run makes the restart button stop working too.

A job that legitimately needs longer should set timeoutMs in its own definition rather than raising this. Config → Jobs says which of the two each job is doing.

**If it's wrong.** Too low and a healthy long job is recorded as a failure, repeatedly, with a duration suspiciously close to the ceiling — that similarity is the tell, and its step trace stops mid-work rather than at an error.

Too high and a hung job stays in flight for as long as the ceiling allows, blocking restarts the whole time. Note that the ceiling is per attempt: three retries of a five-minute job can occupy fifteen minutes plus the waits.

Default: 1800000 ms · Takes effect: immediately · Settings key: `defaultTimeoutMs`

## mail

### Mail account — `RN_MAIL_USER`

**What it does.** The address rn sends from and reads with — the From line on every message the send job produces, and the username for both SMTP and IMAP.

One setting rather than two because it is one account. The password is not here: it is the gmailAppPassword credential, set on Config → Jobs and never shown back.

**Why you would change it.** Nothing else identifies the sender. It is on the outside of every message that arrives, so it is not a secret and holding it as one would only hide it from the page that should say which account is in use.

docs/link-tracking.md §1 takes the app-password path precisely so a single opaque credential covers sending and reading; a pair of user fields that must always match is a pair that can disagree.

**If it's wrong.** Empty and both mail jobs refuse before opening a connection, saying so rather than failing somewhere less legible. Wrong and SMTP rejects the login with a 535 — which rn classifies as permanent, so it fails once instead of three times.

An address that does not match the app password's account is the same 535: the credential is minted for one account and means nothing for another.

Default: unset (system default) · Takes effect: on restart · Settings key: `mailUser`

### Accept mail only from — `RN_MAIL_ALLOWED_SENDERS`

**What it does.** Addresses or domains the read-mail job will accept, one per line or comma-separated. "reports@example.com" is that address exactly; "example.com" or "@example.com" is anybody at that domain.

Empty means every sender. The job's own "Only from these senders" field overrides this for a single run you start by hand.

**Why you would change it.** It has to live here rather than only on the run form, because the scheduler supplies no inputs: an automatic run uses the job's declared defaults, so a filter typed on the form would be empty on every scheduled run — which is every run that matters. A filter that is decorative exactly where it counts is worse than none, because the form implies it is working.

What it buys is not a tidier report. The filter narrows the search on the server, so mail from anyone else is never downloaded, never scanned and never written to a run record — and since this job's output goes on a page and into the job history, not fetching a message is the only way to be sure it is not stored.

**If it's wrong.** A domain here is matched against the parsed sender address and never the display name, and as a suffix on "@domain" rather than a substring — notexample.com contains example.com and anybody can register it. The server's own search is looser than both, so a message that satisfies the server and fails this check is reported as sender-mismatch rather than dropped: that is either a coincidence or somebody putting a trusted address in their display name.

A typo means runs that report nothing, which looks exactly like a quiet inbox. The searched count on each run record tells the two apart.

Default: unset (system default) · Takes effect: on restart · Settings key: `mailAllowedSenders`

### SMTP host — `RN_SMTP_HOST`

**What it does.** The server the send job hands outgoing mail to.

**Why you would change it.** Gmail's by default, because that is the account shape docs/link-tracking.md §1 recommends. Any SMTP server works — the job speaks the protocol, not Gmail.

It is also the host to add to the network allowlist on Config → Connection: under Deno the runtime denies everything not named there, and the failure message talks about permissions without mentioning that an allowlist exists.

**If it's wrong.** A host that does not resolve fails the run with the connection error and sends nothing — no half-send, because the connection is opened before the first recipient. A host that resolves but is not an SMTP server hangs until the job's timeout.

Default: smtp.gmail.com · Takes effect: on restart · Settings key: `smtpHost`

### SMTP port — `RN_SMTP_PORT`

**What it does.** The port, and — because 465 means implicit TLS — also the choice of how the session is encrypted. Set to 465 the connection is TLS from the first byte; set to anything else it is not.

**Why you would change it.** 587 is the common alternative and it is weaker in a specific way: it opens in plaintext and upgrades with STARTTLS, so a network that strips the upgrade leaves the whole session — credentials included — readable. 465 cannot be downgraded that way because there is no plaintext phase to strip.

**If it's wrong.** Point 465 at a server that only speaks STARTTLS and the handshake fails immediately, which is the honest failure. The dangerous direction is the other one: a port that quietly works without encryption looks identical to one that works with it, from here.

Default: 465 · Takes effect: on restart · Settings key: `smtpPort`

### IMAP host — `RN_IMAP_HOST`

**What it does.** The server the read-mail job polls for arriving mail.

**Why you would change it.** The counterpart of the SMTP host, and the same app password authenticates against both — which is the whole reason §1 prefers an app password to OAuth.

Needs adding to the network allowlist on Config → Connection alongside the SMTP host, for the same reason.

**If it's wrong.** The run fails on connect and changes nothing at all — this job never writes to the mailbox, so a wrong host costs a red run and no more.

Default: imap.gmail.com · Takes effect: on restart · Settings key: `imapHost`

### IMAP port — `RN_IMAP_PORT`

**What it does.** The port, and the encryption choice with it: 993 is implicit TLS, the IMAP counterpart of SMTP's 465, and anything else connects in the clear.

**Why you would change it.** Mail bodies and the account password both cross this connection. 143 is the plaintext port and exists for STARTTLS, with the same downgrade weakness 587 has on the sending side.

**If it's wrong.** A mismatch fails the handshake rather than silently reading your mail over an unencrypted socket — but only because the port decides TLS here. Changing this to a non-993 port turns encryption off, so it is not a setting to adjust while chasing a connection problem.

Default: 993 · Takes effect: on restart · Settings key: `imapPort`

## Output

### Disable colored output — `NO_COLOR`

**What it does.** Suppresses ANSI color codes in rn's output.

**Why you would change it.** Logs piped to a file or a viewer that doesn't understand escape codes.

**If it's wrong.** Cosmetic only — no job behaves differently. Turning it on when your terminal does support color makes output harder to scan, not broken.

Default: off · Takes effect: on restart · Settings key: `color`

## Deliberately not exposed

These are decisions, not omissions. Each has a reason a user would want it and a
better reason not to give it to them.

- **`--stack-size`** — Sounds like the companion to the memory limit and is not. It raises V8's call-stack ceiling, but the operating system fixed the real thread stack at about 1MB when the thread started, so setting it higher does not buy deeper recursion — it removes the check that would have raised 'Maximum call stack size exceeded' and lets the process run off the end of its stack instead. A catchable RangeError becomes a segfault with no JavaScript error at all.
- **`--insecure-http-parser`** — Accepts malformed HTTP headers instead of rejecting them. The danger is not leniency in itself, it is disagreement: a proxy in front reads a malformed request one way and this process reads it another, so an attacker can hide a second request inside the first and have it treated as trusted. Nothing rn does needs to parse broken HTTP.
- **`--tls-min-v1.0 / --tls-min-v1.1`** — Lowers the TLS floor to versions with known breaks. Since the peer influences which version gets negotiated, offering an old one means an attacker who can sit in the middle chooses it. A server too old for TLS 1.2 is a reason to fix the server, not to meet it there.
- **`--preload (bun)`** — Bun's alias set for --require and --import, and the same injection vector: it runs arbitrary code before the app does. Withheld for the reason its Node counterpart is.
- **`--inspect / --inspect-brk`** — Opens a debugger port on the user's machine. Anything that can reach it can run code in the process. A gated diagnostic at most, never a checkbox.
- **`--require / --import`** — Preloads arbitrary code. This is the injection vector the launcher's sealed environment exists to block; offering it in the UI reopens the door by hand.
- **`NODE_TLS_REJECT_UNAUTHORIZED`** — Disables certificate validation entirely. Users find it on forums as the fix for a TLS error. Use the Extra CA certificates setting instead.
- **`--no-deprecation`** — Hides warnings. This app exists to make what is happening visible; a control whose purpose is concealment contradicts that.
- **`--jitless, --expose-gc, --prof`** — Developer tools with real costs and no user-comprehensible benefit. --jitless in particular makes everything slower with no visible cause.

## How a change is applied

1. The Config page writes the value into the settings file.
2. `resolveLaunch()` in `be/src/settings.ts` turns stored settings into environment
   variables and a `NODE_OPTIONS` list.
3. The launcher applies those when it spawns Node, on top of a cleared environment.

Everything but the stack trace depth is read once at process start by libuv, ICU or
the TLS stack, so it cannot change in a running process. The UI must say **restart
required** rather than pretending the change took effect.
