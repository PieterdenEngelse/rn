# Node runtime parameters exposed to users

**Generated from `be/src/runtime-params.ts` by `npm run params:build`. Do not edit.**

Node v24.19.0 has 1,035 command-line flags (177 Node, 858 V8) and 19 environment
variables. Almost none of them belong in front of a user. These are the ones that do:
settings someone running rn could plausibly need, understand, and not silently break.

Values were measured on a 4-thread, 7 GB machine — the heap default in particular
scales with installed RAM, so expect a different number elsewhere.

| Setting | Set via | Default | Range | Takes effect |
|---|---|---|---|---|
| **JavaScript runtime** | `runtime` (NODE_OPTIONS) | node | — | on restart |
| **Extra network hosts** | `netAllowlist` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Node version line** | `nodeVersion` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Heap memory limit** | `--max-old-space-size` (NODE_OPTIONS) | unset (system default) | 64 … 32768 MB | on restart |
| **libuv thread pool** | `UV_THREADPOOL_SIZE` | 4 | 1 … 1024 | on restart |
| **New-space size (advanced)** | `--max-semi-space-size` (NODE_OPTIONS) | unset (system default) | 1 … 1024 MB | on restart |
| **Bun low-memory mode** | `--smol` (NODE_OPTIONS) | off | — | on restart |
| **Bun kill orphans** | `--no-orphans` (NODE_OPTIONS) | off | — | on restart |
| **Bun no auto-install** | `--no-install` (NODE_OPTIONS) | off | — | on restart |
| **Deno V8 flags** | `--v8-flags` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Block native addons** | `--no-addons` (NODE_OPTIONS) | off | — | on restart |
| **Deno no remote modules** | `--no-remote` (NODE_OPTIONS) | off | — | on restart |
| **Unhandled rejection policy** | `--unhandled-rejections` (NODE_OPTIONS) | unset (system default) | — | on restart |
| **Time zone** | `TZ` | unset (system default) | — | on restart |
| **Extra CA certificates** | `NODE_EXTRA_CA_CERTS` | unset (system default) | — | on restart |
| **Trace warnings** | `--trace-warnings` (NODE_OPTIONS) | off | — | on restart |
| **Trace deprecations** | `--trace-deprecation` (NODE_OPTIONS) | off | — | on restart |
| **Stack trace depth** | `--stack-trace-limit` (NODE_OPTIONS) | 10 | 0 … 200 | immediately |
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

**What it does.** Which Node major line the app should run on. Unset means 'use whatever is bundled', which is the value in be/.nvmrc — currently v24.19.0.

**Why you would change it.** Pin an older line when a native addon has not been rebuilt for a newer one yet. It buys compatibility at the cost of being on a clock: a maintenance line stops getting fixes before the others do.

**If it's wrong.** The mismatch that actually bites is drift — settings asking for one line while the bundled binary is another. The active row above reports what the process really is, so trust that over this and treat any disagreement as the bug.

Default: unset (system default) · Takes effect: on restart · Settings key: `nodeVersion`

## Memory

### Heap memory limit — `--max-old-space-size`

**What it does.** Caps V8's old-space heap. Unset, Node picks a limit from installed RAM — on this machine that came out at 2240 MB.

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

**If it's wrong.** Too high wastes memory and adds contention. Above 1024 libuv silently clamps — a value of 2000 starts with no warning and behaves as 1024.

The symptom of it being too low is a job that is slow while the machine looks idle: low CPU, low event-loop utilisation, and the Monitor's thread pool figure sitting at its ceiling. That combination is this setting and almost nothing else.

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
