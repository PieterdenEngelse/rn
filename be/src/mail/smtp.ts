/**
 * One SMTP transport, for every job that sends mail.
 *
 * Extracted from `send-mail` when `notify-mail` arrived and needed the same
 * connection with the same settings. Two builders would have been two places
 * for the timeout, the implicit-TLS rule and the credential to drift, and the
 * one that drifted would be the one nobody sends from often enough to notice.
 *
 * The account itself is not an argument: host, port, user and timeouts come
 * from `config`, because they belong to the install. Only the password is
 * passed in — read from `ctx.secret` at the point of use, and held nowhere
 * else.
 */

import { config } from "../config.ts";

export interface Transport {
    send(message: {
        /**
         * A bare address, or a name and address the library encodes into one.
         *
         * The object form rather than a string this file assembles: a display
         * name containing a comma, a quote or anything outside ASCII has to be
         * quoted or RFC 2047 encoded, and a header built by concatenation gets
         * that wrong in a way that shows up in somebody else's mail client.
         */
        from: string | { name: string; address: string };
        /**
         * Where replies should go, when that is not the sending account.
         *
         * Absent rather than empty when unset: an empty `Reply-To` is a header
         * that says replies go nowhere, which some clients honour literally.
         */
        replyTo?: string;
        to: string;
        subject: string;
        html: string;
        text: string;
    }): Promise<{ accepted: string[] }>;
}

/**
 * The real transport, built per run and closed after it.
 *
 * `nodemailer` is imported dynamically so that nothing in the module graph
 * pulls it in for a dry run, a test, or the seventeen other things that import
 * this file's job declaration to render a page. It is the backend's only
 * runtime dependency, and the one place it is loaded is the line before it is
 * used.
 */
export async function smtpTransport(password: string): Promise<Transport & { close(): void }> {
    const { createTransport } = await import("nodemailer");
    const tx = createTransport({
        host: config.smtpHost,
        port: config.smtpPort,
        // 465 is implicit TLS. See config.ts for why not 587.
        secure: config.smtpPort === 465,
        auth: { user: config.mailUser, pass: password },
        // Both, from one setting. nodemailer defaults these to ten and two
        // minutes, and the first of those is the job's own ceiling — so a
        // silent socket presents as the run timing out rather than as the
        // server having stopped answering.
        socketTimeout: config.smtpTimeoutMs,
        connectionTimeout: config.smtpTimeoutMs,
    });
    return {
        async send(message) {
            const info = await tx.sendMail(message);
            // Only which addresses the server took. `info.response` is the raw
            // SMTP reply line and would land in a run record verbatim, so it is
            // deliberately not carried.
            return { accepted: info.accepted.map(String) };
        },
        close() {
            tx.close();
        },
    };
}
