/**
 * How rn proves it may read and send this mailbox: an app password, or a
 * Google sign-in.
 *
 * One place, because there are five: the IDLE watcher, read-mail's probe,
 * read-mail's own connection, the SMTP transport both sending jobs share, and
 * the connection test on the API. Five copies of "password, unless…" is five
 * chances for one of them to go on sending a password after a sign-in, and the
 * failure would read as a wrong password rather than as a route not taken.
 *
 * **Why a sign-in is not merely another way to spell the same thing.** Gmail
 * does not accept an OAuth token as a password: IMAP and SMTP take it through
 * XOAUTH2, a different authentication mechanism, which is why this returns a
 * shape rather than a string. Putting the token in `pass` fails with the same
 * "Invalid credentials" as a typo.
 *
 * **The scope is checked, not assumed.** `googleToken` is a general-purpose
 * token — a sign-in asks for whatever scopes the thing reading it needs, and
 * a token minted for Drive is a perfectly good token that Gmail will refuse.
 * So the mail scope has to be among the ones the sign-in actually granted,
 * and a token without it is reported as such instead of being tried: "not
 * authorised for mail" is a sentence somebody can act on, where the provider's
 * own answer is not.
 *
 * Preferred over the app password when both exist, because Google keeps
 * narrowing where app passwords work and a signed-in token is renewed before
 * a run rather than expiring silently in a file.
 */

import { config } from "../config.ts";
import * as oauth from "../oauth.ts";
import * as secrets from "../secrets.ts";

/** Full IMAP and SMTP access. Gmail has no narrower scope that serves them. */
export const MAIL_SCOPE = "https://mail.google.com/";

/** The app password, as the mail jobs have always declared it. */
export const PASSWORD_CREDENTIAL = "gmailAppPassword";

/** Where a Google sign-in puts its access token. */
export const TOKEN_CREDENTIAL = oauth.providerById("google")!.tokenCredential;

export type MailAuth =
    | { kind: "oauth"; user: string; accessToken: string }
    | { kind: "password"; user: string; pass: string };

/**
 * Which of the two to use, or why neither can be.
 *
 * `read` is the reader: a job passes `ctx.secret` so the declaration stays
 * honest, and the watcher and the API pass `secrets.read` because neither is
 * a job. Either way it must answer `undefined` for a credential that is not
 * set rather than throwing, so this can say which are missing instead of
 * failing on the first one it asks for.
 */
export function chooseAuth(
    read: (name: string) => string | undefined = secrets.read,
    scopes: readonly string[] = oauth.grantedScopes("google"),
): MailAuth | { problem: string } {
    const user = config.mailUser;
    if (user === "") {
        return { problem: "RN_MAIL_USER is not set, so there is no account to authenticate as" };
    }

    const token = read(TOKEN_CREDENTIAL);
    const password = read(PASSWORD_CREDENTIAL);

    if (token !== undefined && scopes.includes(MAIL_SCOPE)) {
        return { kind: "oauth", user, accessToken: token };
    }
    if (password !== undefined) return { kind: "password", user, pass: password };

    if (token !== undefined) {
        return {
            problem:
                `${TOKEN_CREDENTIAL} is set, but that sign-in did not grant ${MAIL_SCOPE} — ` +
                `sign in to Google again asking for it, or set ${PASSWORD_CREDENTIAL}`,
        };
    }
    return {
        problem:
            `neither ${PASSWORD_CREDENTIAL} nor a Google sign-in granting ${MAIL_SCOPE} is ` +
            "configured, and one of the two is what the mailbox is opened with",
    };
}

/** `imapflow`'s auth block: a password, or a token through XOAUTH2. */
export function imapAuth(auth: MailAuth): { user: string; pass: string } | { user: string; accessToken: string } {
    return auth.kind === "oauth"
        ? { user: auth.user, accessToken: auth.accessToken }
        : { user: auth.user, pass: auth.pass };
}

/**
 * `nodemailer`'s auth block.
 *
 * `type: "OAuth2"` with an access token and nothing else: nodemailer can run
 * the refresh itself given a client id, secret and refresh token, and it
 * deliberately is not given them. Renewal happens once, before the run, under
 * the lock in oauth.ts — a second renewer inside the mailer would spend the
 * same rotating refresh token from a place that does not know about the first.
 */
export function smtpAuth(
    auth: MailAuth,
): { user: string; pass: string } | { type: "OAuth2"; user: string; accessToken: string } {
    return auth.kind === "oauth"
        ? { type: "OAuth2", user: auth.user, accessToken: auth.accessToken }
        : { user: auth.user, pass: auth.pass };
}

/** One line for a page or a run record: how this mailbox is being opened. */
export function describeAuth(
    read: (name: string) => string | undefined = secrets.read,
    scopes: readonly string[] = oauth.grantedScopes("google"),
): { kind: "oauth" | "password" | "none"; detail: string } {
    const chosen = chooseAuth(read, scopes);
    if ("problem" in chosen) return { kind: "none", detail: chosen.problem };
    return chosen.kind === "oauth"
        ? {
              kind: "oauth",
              detail: `XOAUTH2 with ${TOKEN_CREDENTIAL}, from a Google sign-in granting ${MAIL_SCOPE}`,
          }
        : { kind: "password", detail: `the ${PASSWORD_CREDENTIAL} credential` };
}

/**
 * The same choice, for a job.
 *
 * `ctx.secret` throws on a credential that is not set, which is right for a
 * job reading one it needs and wrong for asking which of two exist. So the
 * question "is it set" goes to `secrets.isSet` and only a credential that is
 * there is read — and reading it still goes through `ctx.secret`, so a job
 * that forgot to declare the name throws by name rather than quietly working.
 */
export function authForJob(
    // `string | undefined`, because a probe's accessor answers that where a
    // run's throws — and this is the one caller that asks about a credential
    // it may not have.
    ctx: { secret(name: string): string | undefined },
): MailAuth | { problem: string } {
    return chooseAuth((name) => (secrets.isSet(name) ? ctx.secret(name) : undefined));
}
