/**
 * Which credential opens the mailbox, and what the other four callers are
 * spared from deciding for themselves.
 *
 * The property under test is not "OAuth works" — that needs Gmail — but that
 * the *choice* is one function with one answer: the watcher, read-mail's
 * probe, read-mail, the SMTP transport and the connection test on the API all
 * ask it, and a page that says "signed in with Google" while a run sends an
 * app password would be two statements about one mailbox.
 *
 * The scope check is the subtle half. `googleToken` is a general-purpose
 * token, and one minted for Drive is a perfectly good token Gmail refuses —
 * so a token without the mail scope must be reported, not tried.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { config } from "../src/config.ts";

(config as unknown as { mailUser: string }).mailUser = "person@example.com";

const { chooseAuth, describeAuth, imapAuth, smtpAuth, MAIL_SCOPE, PASSWORD_CREDENTIAL, TOKEN_CREDENTIAL } =
    await import("../src/mail/auth.ts");

/** A reader over a plain object, standing in for the credentials file. */
const reader = (held: Record<string, string>) => (name: string) => held[name];

const PASSWORD = "sixteen-letters-x";
const TOKEN = "ya29.a0-the-access-token";

test("a signed-in token with the mail scope wins over the app password", () => {
    const chosen = chooseAuth(reader({ [PASSWORD_CREDENTIAL]: PASSWORD, [TOKEN_CREDENTIAL]: TOKEN }), [
        MAIL_SCOPE,
    ]);
    assert.deepEqual(chosen, { kind: "oauth", user: "person@example.com", accessToken: TOKEN });
    // XOAUTH2, not a password in another wrapper: put the token in `pass` and
    // Gmail answers exactly as it does for a typo.
    assert.deepEqual(imapAuth(chosen as never), { user: "person@example.com", accessToken: TOKEN });
    assert.deepEqual(smtpAuth(chosen as never), {
        type: "OAuth2",
        user: "person@example.com",
        accessToken: TOKEN,
    });
});

test("a token scoped for something else is named, not tried", () => {
    const held = { [TOKEN_CREDENTIAL]: TOKEN };
    const chosen = chooseAuth(reader(held), ["https://www.googleapis.com/auth/drive.readonly"]);
    assert.ok("problem" in chosen);
    assert.match(chosen.problem, /did not grant https:\/\/mail\.google\.com\//);

    // With a password beside it, that is simply the route taken — a narrow
    // token is not a reason to stop reading mail.
    const both = chooseAuth(reader({ ...held, [PASSWORD_CREDENTIAL]: PASSWORD }), ["openid"]);
    assert.deepEqual(both, { kind: "password", user: "person@example.com", pass: PASSWORD });
});

test("with neither, the refusal names both routes rather than one", () => {
    const chosen = chooseAuth(reader({}), []);
    assert.ok("problem" in chosen);
    assert.match(chosen.problem, /gmailAppPassword/);
    assert.match(chosen.problem, /mail\.google\.com/);
    assert.equal(describeAuth(reader({}), []).kind, "none");
});

test("no address is a refusal before either credential is looked at", () => {
    const user = config.mailUser;
    (config as unknown as { mailUser: string }).mailUser = "";
    try {
        const chosen = chooseAuth(reader({ [PASSWORD_CREDENTIAL]: PASSWORD }), [MAIL_SCOPE]);
        assert.ok("problem" in chosen);
        assert.match(chosen.problem, /RN_MAIL_USER/);
    } finally {
        (config as unknown as { mailUser: string }).mailUser = user;
    }
});

test("what the page is told says which route, and never the value", () => {
    const oauth = describeAuth(reader({ [TOKEN_CREDENTIAL]: TOKEN }), [MAIL_SCOPE]);
    assert.equal(oauth.kind, "oauth");
    const password = describeAuth(reader({ [PASSWORD_CREDENTIAL]: PASSWORD }), []);
    assert.equal(password.kind, "password");
    for (const line of [oauth.detail, password.detail]) {
        assert.ok(!line.includes(TOKEN) && !line.includes(PASSWORD), line);
    }
});
