/**
 * Tell a runtime permission refusal apart from an ordinary network failure.
 *
 * One function, used by every job that reaches the network. It was two — an
 * identical regex in `watch-upstreams` and in `watch-feeds`, differing only in
 * which hosts the message named — and they were kept apart on purpose until the
 * refusal path had been *watched* rather than inferred. Sharing a helper that
 * nothing had ever executed would have made one unverified thing look like two.
 *
 * ## Why a job needs this at all
 *
 * Selecting Deno as the runtime turns `netAllowlist` from a convention into an
 * enforced permission: `launcher/src/layout.rs` renders it as
 * `--allow-net=host,host` and Deno denies everything not named. So a job that
 * fetches is the first thing in rn a runtime switch can break, and it breaks
 * with a message about permissions that says nothing about where to grant them.
 * Without this the run reports a bare `Requires net access` and the person
 * reading it has no reason to think an allowlist exists.
 *
 * ## What was actually observed
 *
 * On deno 2.9.5, against a scratch backend granted only rn's own two ports,
 * every lookup failed with:
 *
 *     Requires net access to "nodejs.org:443", run again with the --allow-net flag
 *
 * Not `PermissionDenied`, not `NotCapable`. **All three arms stay anyway.** That
 * is Deno's wording to change, and the two failure modes are not symmetric: an
 * arm that never matches costs nothing, while a hint that quietly stops firing
 * leaves the bare error on the record and takes the allowlist with it. The
 * advice was checked too, not just the trigger — those hosts in `netAllowlist`
 * plus a restart takes the same run to a clean report.
 *
 * ## Hosts are an argument
 *
 * Named by the caller, from the hosts that run was actually going to reach.
 * The version this grew out of hardcoded three, which was wrong the moment a
 * run narrowed itself: asking `watch-upstreams` for npm alone and being told to
 * allowlist nodejs.org and crates.io is advice about two hosts it never touched.
 */

/** Deno's own names for the two error classes a refused grant produces. */
const DENIAL_NAMES = new Set(["permissiondenied", "notcapable"]);

/**
 * Is this a runtime refusal, as opposed to an error that merely contains the
 * word somewhere?
 *
 * Anchored rather than searched, and the difference is not academic. Both jobs
 * put the URL they failed on into the message — `404 Not Found from
 * https://…` — and a URL is data: a feed address is typed by the user, and a
 * crates.io lookup carries whatever a manifest names. A bare substring test
 * over that message reported a permission problem for an ordinary 404 on
 * `https://blog.example/tags/permissiondenied.atom`, which is a plausible feed
 * rather than a contrived one.
 *
 * It matters more than a wrong sentence, because a caller may treat this
 * firing as a *classification* — a permission grant is fixed when the process
 * starts, so a refusal will not improve on a retry, while the 503 underneath a
 * false positive is exactly what a retry is for. Getting this wrong that way
 * turns a transient failure into one that is never tried again.
 *
 * So: the error's own name, or the wording at the start of the message where a
 * runtime puts its class, or the phrase Deno actually produces — which carries
 * a space and a "to", and so cannot arrive inside a URL.
 *
 * The name arm is the one to protect. It is the only test a URL cannot spoof,
 * and it is also the only one that survives Deno rewording its message — which
 * is the change all three arms exist to be ready for. It only works if callers
 * hand over what was *thrown* rather than the string it flattens to: both jobs
 * keep the error beside its message for exactly this, and a caller that passes
 * `err.message` silently gets the two weaker arms and no warning.
 */
function isDenial(name: string, message: string): boolean {
    return (
        DENIAL_NAMES.has(name.trim().toLowerCase()) ||
        /^\s*(?:PermissionDenied|NotCapable)\b/i.test(message) ||
        /\brequires net access to\b/i.test(message)
    );
}

/** "a", "a and b", "a, b and c" — the message reads as a sentence either way. */
function readable(hosts: readonly string[]): string {
    const unique = [...new Set(hosts)].filter((h) => h !== "");
    if (unique.length === 0) return "the host it needs";
    if (unique.length === 1) return unique[0]!;
    return `${unique.slice(0, -1).join(", ")} and ${unique.at(-1)!}`;
}

/**
 * The advice to add to a network failure, or `undefined` if it is not one of
 * these.
 *
 * `undefined` rather than an empty string, so a caller cannot append it and
 * quietly produce a message with a trailing full stop and nothing after it.
 */
export function netPermissionHint(err: unknown, hosts: readonly string[]): string | undefined {
    const name = err instanceof Error ? err.name : "";
    const message = err instanceof Error ? err.message : String(err);
    if (!isDenial(name, message)) return undefined;
    return (
        `The runtime refused the outbound request. Under Deno, add ${readable(hosts)} to the ` +
        "network allowlist on Config → Connection and restart — the launcher grants Deno " +
        "only rn's own addresses by default."
    );
}
