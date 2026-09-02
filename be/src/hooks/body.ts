/**
 * Turning the bytes a delivery carried into something a job can read.
 *
 * Separate from the listener and pure, like `verify.ts` beside it, because this
 * runs on input from anyone who holds the secret and the rules are worth
 * reading in one place rather than inferred from a chain of `if`s inside a
 * request handler.
 *
 * **Why this exists at all.** The listener used to call `JSON.parse` on
 * whatever arrived, whatever the sender said it was. A form-encoded body —
 * Slack's slash commands, and several older providers — therefore verified its
 * signature and was *then* refused as malformed. The delivery was authentic and
 * the failure looked exactly like a wrong secret, which is where anyone would
 * have gone looking first.
 *
 * The bytes are parsed only after the signature has been checked, and this
 * module never sees them before that. The order matters more than the parsing:
 * see `handle` in `server.ts`.
 */

import type { JsonValue } from "../generated/serde_json/JsonValue.ts";

/**
 * 400 and 415 are different answers on purpose.
 *
 * 415 says "I do not read this kind of thing", 400 says "this is the right kind
 * and it is broken". A provider debugging a failed delivery can act on the
 * first without touching their payload, and on the second without touching
 * their headers. One code for both would have made every content-type mistake
 * look like a malformed body — the failure this module was written to remove,
 * reintroduced one layer up.
 */
export type ParseOutcome =
    | { ok: true; value: JsonValue }
    | { ok: false; code: 400 | 415; reason: string };

/**
 * The media type alone: no charset, no boundary, lowercased.
 *
 * `application/json; charset=utf-8` is the common spelling and must not be a
 * different type from `application/json`. Encoding is not honoured beyond
 * this — the bytes are read as UTF-8 whatever a `charset` claims, because
 * every provider in question sends UTF-8 and guessing at the rest is how a
 * body gets silently mangled.
 */
export function mediaType(header: string | undefined): string {
    if (header === undefined) return "";
    const semi = header.indexOf(";");
    return (semi === -1 ? header : header.slice(0, semi)).trim().toLowerCase();
}

/** `application/vnd.github+json` and friends are JSON, and say so in the suffix. */
function isJson(type: string): boolean {
    return type === "application/json" || type.endsWith("+json");
}

/**
 * Repeated keys become an array, a single key stays a string.
 *
 * Both shapes are real in form encoding — `a=1&a=2` is a list and `a=1` is a
 * value — and collapsing them to one would mean either wrapping every scalar in
 * a one-element array or silently dropping the second value. A job reading
 * `ctx.payload.a` gets what was sent, and finds out it is a list by looking.
 */
function fromForm(text: string): JsonValue {
    const params = new URLSearchParams(text);
    const out: Record<string, JsonValue> = {};
    for (const key of new Set(params.keys())) {
        const all = params.getAll(key);
        const [first] = all;
        out[key] = all.length === 1 && first !== undefined ? first : all;
    }
    return out;
}

export function parseBody(raw: Buffer, contentType: string | undefined): ParseOutcome {
    // An empty body is an empty object rather than an error, which is what the
    // listener did before this module existed. Providers send one for ping and
    // health deliveries, and a job that reads nothing out of the payload should
    // not care which it got.
    if (raw.length === 0) return { ok: true, value: {} };

    const type = mediaType(contentType);
    const text = raw.toString("utf8");

    // No content-type at all is treated as JSON. It is what the listener has
    // always assumed, so a provider that omits the header keeps working; and
    // the alternative — 415 for a header nobody sent — would break deliveries
    // that verify perfectly well.
    if (type === "" || isJson(type)) {
        try {
            return { ok: true, value: JSON.parse(text) as JsonValue };
        } catch {
            return { ok: false, code: 400, reason: "body is not valid JSON" };
        }
    }

    if (type === "application/x-www-form-urlencoded") {
        return { ok: true, value: fromForm(text) };
    }

    // Anything textual arrives as a string under one key rather than as a bare
    // string. A job reading `ctx.payload.text` reads the same shape it reads
    // for every other content type: an object.
    if (type.startsWith("text/")) {
        return { ok: true, value: { text } };
    }

    return { ok: false, code: 415, reason: `unsupported content type: ${type}` };
}
