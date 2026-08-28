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
    const message = err instanceof Error ? `${err.name}: ${err.message}` : String(err);
    if (!/PermissionDenied|Requires net access|NotCapable/i.test(message)) return undefined;
    return (
        `The runtime refused the outbound request. Under Deno, add ${readable(hosts)} to the ` +
        "network allowlist on Config → Connection and restart — the launcher grants Deno " +
        "only rn's own addresses by default."
    );
}
