/**
 * A failure that will fail the same way in fifteen seconds.
 *
 * The runner's retry loop is generic and right to be: a job says
 * `retry: { attempts: 3, backoffMs: 15_000 }` and means it about the failures
 * it was written for — a 502 from a relay, a phone off wifi, a registry having
 * a bad minute. All of those are answered by asking again.
 *
 * Some are not, and the runner cannot tell which from the outside. A receiver
 * that says `invalid_payload` will say it again. A format string with a typo in
 * it is still a typo on the third attempt. A Deno permission grant is fixed
 * when the process starts and cannot widen while it runs. Retrying those costs
 * ninety seconds to conclude what was known immediately, puts three identical
 * errors on the record, and holds the parent job in flight for the whole of it,
 * because `onChange` is awaited.
 *
 * ## Why the job marks it, rather than the runner asking
 *
 * The two shapes considered were a predicate the runner calls with each error,
 * and this: the job throws something the runner recognises. The predicate loses
 * on the evidence. In every real case here the job has *already* classified the
 * failure at the point it throws — `notify` reads `res.status`, and both watch
 * jobs have just called `netPermissionHint()`, whose returning a string is the
 * classification. A predicate would hand a job's own error back to it so it
 * could re-derive, from a formatted message string, what it knew one line
 * earlier. That is a parser where an `if` will do, and it fails quietly when
 * the wording of the message changes.
 *
 * ## What it is not
 *
 * Not `UnretryableError` in `run.ts`, which is the runner's own and means
 * something entirely different: the attempt timed out and the work never
 * stopped, so a retry would run a second copy alongside the first. That one is
 * about safety and is the runner's to decide. This one is about pointlessness
 * and is the job's.
 *
 * Not a way to turn retries off. `retry` stays declared and a transient failure
 * in the same job still gets its three attempts — this marks one error, not one
 * job.
 *
 * ## Using it
 *
 *     if (isPermanentStatus(res.status)) {
 *         throw new PermanentFailure(message, "the receiver rejected the request itself");
 *     }
 *
 * The `reason` is written into the run's step trace beside the error, and it is
 * the answer to "why did my `retry: 3` job only try once" — which has to have
 * one, the same way `retry-abandoned` did.
 */
export class PermanentFailure extends Error {
    /**
     * Why asking again cannot help. One clause, in the same voice as the error
     * itself, because the two are read together in the trace.
     */
    readonly reason: string;

    constructor(message: string, reason: string) {
        super(message);
        this.name = "PermanentFailure";
        this.reason = reason;
    }
}

/**
 * Whether an HTTP status is the receiver refusing this request as such.
 *
 * 4xx is the sender's fault by definition, and sending the identical bytes
 * again is answered identically — with two exceptions that are 4xx by numbering
 * and transient by meaning. 408 is the server saying it gave up waiting, which
 * is the plainest possible invitation to try again; 429 is it asking for
 * exactly that, later. 5xx is not here at all: a 502 or a 503 is the failure
 * the retry policy was written for.
 */
export function isPermanentStatus(status: number): boolean {
    if (status === 408 || status === 429) return false;
    return status >= 400 && status < 500;
}
