/**
 * Is this base URL fit to put in mail that will outlive every decision here?
 *
 * Every other mistake in rn is a revert. This one is not: a tracked link lives
 * in a mailbox for as long as the recipient keeps the message, so the base URL
 * can only ever be corrected for mail *not yet sent*. `docs/link-tracking.md`
 * §3 makes that argument for the destination; this module is the same argument
 * applied to the origin, which is the half that was written down as advice and
 * never checked.
 *
 * ## Why a checker rather than a paragraph
 *
 * The failure mode is that everything works. A loopback base URL resolves
 * perfectly on the machine that minted it, the page shows the click, and the
 * links are dead for every recipient. A borrowed hostname is worse still,
 * because it works for *everyone* — right up until the machine is renamed and
 * every link ever sent stops resolving at once, with no way to redirect them.
 * Neither is visible from inside rn, and neither fails a test that does not
 * exist. So the check runs at the one moment it can still matter: before a link
 * is minted.
 *
 * ## What it cannot know
 *
 * Whether a domain is *yours*. Nothing here can tell `links.example.com` from a
 * name somebody else pointed at you. [`BORROWED_SUFFIXES`] is a list of
 * providers whose names are definitionally not yours, so a clean verdict means
 * "no known problem" and never "checked and owned". A free subdomain from a
 * provider not on that list passes and should not.
 */

/**
 * Suffixes that hand out names rather than sell them.
 *
 * A name under one of these is lent, not owned: it is tied to an account, a
 * machine name or a process, and it goes away or moves when any of those do.
 * That is fine for a webhook endpoint a provider re-reads on every delivery,
 * and wrong for a URL printed into somebody's mail.
 *
 * Named individually rather than matched by shape because there is no shape —
 * `ts.net` and `example.com` are the same kind of string, and the difference is
 * a fact about who runs the registry.
 */
export const BORROWED_SUFFIXES = [
    // Tailscale Funnel: the node's own DNS name, which follows the machine.
    ".ts.net",
    // Cloudflare quick tunnels. A *named* tunnel on your own domain is fine —
    // it is this throwaway hostname that is not.
    ".trycloudflare.com",
    ".cfargotunnel.com",
    ".ngrok.io",
    ".ngrok.app",
    ".ngrok-free.app",
    ".loca.lt",
    ".serveo.net",
    ".localhost.run",
    ".lhr.life",
    ".bore.pub",
    ".pagekite.me",
    ".devtunnels.ms",
] as const;

/** Mirrors `BaseUrlProblem` in `shared/src/links.rs`. */
export type BaseUrlProblem =
    | "malformed"
    | "insecure"
    | "loopback"
    | "ip-literal"
    | "port"
    | "borrowed";

/** True for `127.0.0.0/8`, `localhost` and `::1`. */
function isLoopback(hostname: string): boolean {
    const h = hostname.toLowerCase();
    if (h === "localhost" || h.endsWith(".localhost")) return true;
    if (h === "[::1]" || h === "::1") return true;
    return /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(h);
}

/** True for a dotted-quad or a bracketed IPv6 address. */
function isIpLiteral(hostname: string): boolean {
    if (hostname.startsWith("[")) return true;
    return /^\d{1,3}(\.\d{1,3}){3}$/.test(hostname);
}

/**
 * Everything wrong with `base`, in the order a reader should fix it.
 *
 * A list rather than the worst one: the shipped default trips three at once,
 * and reporting only the first would have an operator fix the scheme and see
 * the same complaint again. Empty means no *known* problem — see the module
 * note on what that does not prove.
 */
export function classifyBaseUrl(base: string): BaseUrlProblem[] {
    let url: URL;
    try {
        url = new URL(base);
    } catch {
        return ["malformed"];
    }

    const problems: BaseUrlProblem[] = [];
    if (url.protocol !== "https:" && url.protocol !== "http:") return ["malformed"];
    if (url.protocol === "http:") problems.push("insecure");

    const host = url.hostname;
    if (isLoopback(host)) {
        problems.push("loopback");
    } else if (isIpLiteral(host)) {
        problems.push("ip-literal");
    } else if (BORROWED_SUFFIXES.some((s) => host.toLowerCase().endsWith(s))) {
        problems.push("borrowed");
    }

    // WHATWG URL drops a default port, so this is only ever an explicit
    // non-default one — exactly the thing that reads as phishing in a mail.
    if (url.port !== "") problems.push("port");

    return problems;
}

/**
 * Throw unless `base` is fit to mint, or the caller has said it knows better.
 *
 * Separate from [`classifyBaseUrl`] so the page can report a problem without
 * anything throwing, and the mint path can refuse without deciding what to say
 * about it.
 */
export function assertMintableBase(base: string, allowUnsafe = false): void {
    if (allowUnsafe) return;
    const problems = classifyBaseUrl(base);
    if (problems.length === 0) return;
    throw new Error(
        `refusing to mint tracked links against ${base}: ${problems.join(", ")}. ` +
            "A tracked link outlives every setting here — it stays in the recipient's " +
            "mailbox — so this cannot be corrected after the mail is sent. Set " +
            "RN_TRACKER_BASE_URL to an https origin on a domain you own, or pass " +
            "allowUnsafeBase for a dry run that is not going anywhere.",
    );
}
