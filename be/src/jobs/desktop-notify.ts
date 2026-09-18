/**
 * Raise a notification on the screen of whoever is logged in here.
 *
 * The local counterpart of `notify`. That one posts to a URL and lets the
 * receiver decide what a notification means — a phone, a Slack channel,
 * whatever you point it at. This one draws on the desktop of the machine rn is
 * running on, over whatever program has focus, with no account and no third
 * party in the path.
 *
 * ## The first job here that runs a local program
 *
 * Everything else in `be/src/jobs/` either speaks HTTP or writes a file. This
 * one executes `/usr/bin/notify-send`, so it takes the same care the launcher
 * takes with Node, for the same reasons:
 *
 * - **An absolute path**, never a name resolved through `PATH`. The launcher
 *   grants the child a minimal `PATH` precisely so that what it runs is not a
 *   question the environment gets to answer, and a job that undid that by
 *   spawning `notify-send` by name would be reintroducing the problem one level
 *   up.
 * - **`execFile`, never `exec`**, so there is no shell. Arguments are passed as
 *   an array and reach the program as written. This matters more here than
 *   almost anywhere else in rn: the title and body are built from mail sent by
 *   somebody else, and a shell in that path would be command injection by way
 *   of a subject line.
 * - **Bounded and timed**, because a notification daemon that has wedged must
 *   not hold a job in flight and, through it, the runner's restart.
 *
 * ## Colour, and what is actually available
 *
 * Measured on this machine rather than reasoned about, after two wrong
 * attempts. `xfce4-notifyd` advertises `body-markup` and no colour hint, so
 * `--hint string:fgcolor:` does nothing; and of the markup it does render, `<b>`
 * works while `<span foreground=…>` is dropped. Coloured *text* is therefore not
 * available on this desktop however it is asked for.
 *
 * What is available is the title, because an emoji is a colour glyph drawn by
 * the font and no stylesheet can grey it out — hence a title prefix carrying the
 * default. An icon would work too, and `dialog-error` is the reliably red one in
 * a standard theme, which is exactly why it is *not* the default: it means
 * error, and a mail notification that claims something went wrong teaches you to
 * dismiss the ones that did.
 *
 * ## What it needs, and what it says when it does not have it
 *
 * `DBUS_SESSION_BUS_ADDRESS`, passed through the launcher's seal deliberately —
 * `launcher/src/main.rs` carries the reasoning and `docs/sec.md` records the
 * cost. Without it there is no session bus to talk to, which is the ordinary
 * state of a machine with nobody logged in graphically. That is reported as a
 * skipped run rather than a failure: nothing is wrong, there is simply no
 * screen to draw on, and a red run every thirty minutes on a headless box would
 * be noise that teaches people to ignore red runs.
 */

import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { outcome } from "./history.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";
import type { JobRun } from "../generated/wire.ts";

/**
 * Absolute, and checked rather than assumed.
 *
 * Two paths because distributions disagree and both are ordinary; the first
 * that exists wins. Not resolved through `PATH` — see the header.
 */
const CANDIDATES = ["/usr/bin/notify-send", "/bin/notify-send"];

/** How long to wait for the notification daemon before giving up. */
const TIMEOUT_MS = 10_000;

/**
 * Ceiling on the body, in characters.
 *
 * A notification is a glance, and a daemon handed four kilobytes of mail either
 * truncates it somewhere unhelpful or draws a panel across the screen. Twenty
 * lines is more than anybody reads standing up.
 */
const MAX_BODY = 800;

/**
 * Escape text that is about to sit inside Pango markup.
 *
 * Not optional. The body is built from mail somebody else wrote, and a subject
 * line containing `<` or `&` would either break the markup or inject tags into
 * it. Pango is not a shell and this is not code execution — the worst case is a
 * mangled or attacker-styled notification — but "somebody else's text in a
 * markup context, unescaped" is a habit worth not having.
 */
export function escapeMarkup(text: string): string {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** The first `notify-send` that actually exists, or undefined. */
export function findNotifySend(): string | undefined {
    return CANDIDATES.find((p) => existsSync(p));
}

/**
 * Title and body, from the run that triggered this one.
 *
 * Exported for the tests: what a notification says is the whole product of this
 * job, and it should be checkable without a desktop.
 */
export function buildNotification(cause: JobRun | undefined): {
    title: string;
    body: string;
} {
    if (cause === undefined) {
        // Run by hand, which is how you find out whether notifications work at
        // all. Saying it is a test matters: one that arrives looking like real
        // news, and is not, teaches you to distrust the next one.
        return {
            title: "rn: test notification",
            body: "You pressed Run now. Nothing happened — this is rn checking it can reach your desktop.",
        };
    }

    const lines: string[] = [];
    for (const [k, v] of Object.entries(cause.summary)) lines.push(`${k}: ${String(v)}`);

    // The steps a person actually wants: what arrived, and the links in it.
    // Every other step is machinery, and a notification is not the place to
    // read machinery.
    for (const s of cause.steps) {
        if (s.name === "message") {
            const d = s.detail;
            lines.push(`\n${String(d["from"] ?? "?")}`);
            lines.push(String(d["subject"] ?? "(no subject)"));
        }
        if (s.name === "link") lines.push(String(s.detail["url"] ?? ""));
    }

    const body = lines.join("\n").slice(0, MAX_BODY);
    return { title: `rn: ${cause.jobId} — ${outcome(cause)}`, body };
}

export const desktopNotify: Job = {
    id: "desktop-notify",
    label: "Notify on this desktop",
    source: import.meta.filename,

    // One short call to a local daemon. Short on purpose: this runs while the
    // job that triggered it is still registered in flight, so a restart waits
    // behind it.
    timeoutMs: 20_000,

    inputs: [
        {
            id: "urgency",
            label: "Urgency",
            type: "text",
            // Back to normal now that the colour comes from markup rather
            // than from urgency. critical was only ever chosen because it was
            // the one lever that changed how xfce4-notifyd drew a
            // notification; it also stops the thing auto-dismissing, which is
            // a separate decision and the wrong default for routine mail.
            default: "normal",
            info: {
                what:
                    "How insistent the notification is: low, normal, or critical. Passed " +
                    "straight to notify-send.",
                why:
                    "critical is the one that behaves differently rather than merely looking " +
                    "different — most desktops refuse to auto-dismiss it and show it even in " +
                    "do-not-disturb. That is right for something you must not miss and wrong " +
                    "for anything routine, because a notification that will not go away " +
                    "teaches people to dismiss the next one without reading it.",
                ifWrong:
                    "An unrecognised value is refused before anything is sent, with the three " +
                    "valid ones named. Nothing is lost — fix it and run again.",
            },
        },
        {
            id: "prefix",
            label: "Title prefix",
            type: "text",
            default: "🔴 ",
            info: {
                what:
                    "Put in front of the notification's title. The default is a red dot and " +
                    "a space; empty adds nothing.",
                why:
                    "It is the reliable way to get colour into a notification on this " +
                    "machine. Measured rather than assumed: xfce4-notifyd renders <b> from " +
                    "the body but drops <span foreground=…>, so coloured *text* is not " +
                    "available here however it is asked for. An emoji is a colour glyph drawn " +
                    "by the font, so no stylesheet can grey it out.\n\n" +
                    "A dot rather than a warning sign, and no icon by default, because a red " +
                    "notification should not also claim something went wrong — see the icon " +
                    "field.",
                ifWrong:
                    "A prefix long enough to push the real title out of view defeats the " +
                    "point: the title is the line you read at a glance. One or two characters " +
                    "and a space.\n\n" +
                    "If it shows as a hollow box, the font has no glyph for it. Any character " +
                    "works — a plain ! costs nothing and always renders.",
            },
        },
        {
            id: "icon",
            label: "Icon",
            type: "text",
            default: "",
            info: {
                what:
                    "A named icon from the desktop's theme, shown beside the text. Empty — " +
                    "the default — shows none. \"mail-unread\" and \"dialog-information\" " +
                    "are the sensible ones; \"dialog-error\" is the red one.",
                why:
                    "Empty by default deliberately, and dialog-error is the reason. It is the " +
                    "only reliably red icon in a standard theme, and it *means* error — so " +
                    "using it to say \"you have mail\" makes every arrival look like a " +
                    "failure. A week of that and the notifications that really are failures " +
                    "get dismissed with the rest, which costs more than the colour is worth.\n\n" +
                    "The title prefix gives the same red without the claim, which is why it " +
                    "is the field with a default and this one is not.",
                ifWrong:
                    "A name the theme does not have shows no icon rather than failing — " +
                    "notify-send does not check, and neither does rn. Names come from the " +
                    "freedesktop icon-naming spec; the theme decides what it actually has.",
            },
        },
        {
            id: "expireSeconds",
            label: "Dismiss after",
            type: "number",
            default: 0,
            info: {
                what:
                    "Seconds before the notification disappears on its own. 0 leaves it to " +
                    "the daemon's own default, which on xfce4-notifyd is a few seconds.\n\n" +
                    "Either way it can always be dismissed by hand: click the notification, " +
                    "or the × that appears when the pointer is over it.",
                why:
                    "A notification you have to dismiss is a small tax, and one that vanishes " +
                    "before you look up is no notification at all. Which of those is worse " +
                    "depends on what it is telling you, which is why this is a setting rather " +
                    "than a decision made here.\n\n" +
                    "Note that urgency overrides it: a critical notification does not expire " +
                    "on most desktops however this is set, and that is the daemon's rule " +
                    "rather than something rn can ask it to bend.",
                ifWrong:
                    "Set this long and forget it and notifications stack up on screen until " +
                    "you clear them by hand — mail from one correspondent is fine, a chatty " +
                    "mailing list is not.\n\n" +
                    "If it never disappears whatever you set, check the urgency: critical is " +
                    "the likely reason, and it is doing what it is for.",
            },
        },
        {
            id: "color",
            label: "Colour",
            type: "text",
            default: "#b00020",
            info: {
                what:
                    "A hex colour for the notification's text. Empty sends the body as plain " +
                    "text with no markup at all.",
                why:
                    "Applied as Pango markup in the body — <span foreground=…> — rather than " +
                    "as the fgcolor and frcolor hints, which is the obvious approach and does " +
                    "not work. Asking this machine's daemon what it supports settles it: " +
                    "xfce4-notifyd advertises body-markup and does not advertise any colour " +
                    "hint, so it themes the frame and background itself and colours only what " +
                    "the body asks for.\n\n" +
                    "The text is escaped before the markup is wrapped round it. The body is " +
                    "built from mail somebody else wrote, and an unescaped subject line " +
                    "containing < or & would break the markup or inject tags into it.",
                ifWrong:
                    "An unrecognised value is passed through rather than validated — the " +
                    "daemon is the authority on what it accepts, and refusing a colour rn " +
                    "merely failed to recognise would be rn having an opinion it cannot " +
                    "support.\n\n" +
                    "Only the text is coloured. The frame and the background belong to the " +
                    "notification theme, which is set in XFCE's own Notifications dialog and " +
                    "not from here — so a red *panel* is that dialog's business and a red " +
                    "*message* is this field's.\n\n" +
                    "A daemon that does not support body markup is meant to strip the tags. " +
                    "Clear this field if one shows them literally instead.",
            },
        },
    ],

    info: {
        what:
            "Raises a notification on the desktop of whoever is logged in on this machine, " +
            "using notify-send. Wire it to a job with onChange or onFailure and it reports " +
            "what that job found: for arriving mail, the sender, the subject and the links.\n\n" +
            "Run by hand it sends a test notification saying so, which is how you check it " +
            "reaches your screen before relying on it.",
        why:
            "The local answer to \"tell me\", where the notify job is the remote one. No " +
            "account, no service, no URL that is a bearer capability — and it appears over " +
            "whatever program you are in rather than inside an app you have to be watching.\n\n" +
            "What it costs is a variable through the launcher's environment seal: rn hands " +
            "its Node child almost nothing, and this needs the session bus address to reach " +
            "your notification daemon at all. That is a real widening and docs/sec.md " +
            "records it rather than leaving a hole in the seal for somebody to find.",
        ifWrong:
            "With nobody logged in graphically there is no session bus, and the run is " +
            "SKIPPED rather than failed — nothing is wrong, there is simply no screen. A " +
            "red run every half hour on a headless machine would be noise that teaches " +
            "people to ignore red runs.\n\n" +
            "It only ever reaches this machine. Away from your desk it tells you nothing, " +
            "and that is the trade against the notify job, which reaches a phone but needs a " +
            "service to do it.\n\n" +
            "The title and body are built from mail somebody else wrote. They are passed to " +
            "notify-send as arguments through execFile with no shell anywhere in the path, " +
            "so a subject line is text and never a command.",
        stages: [
            {
                name: "Check the inputs",
                lead: "Urgency, colour, prefix, icon and expiry — settled before anything is drawn.",
                body:
                    "Urgency must be low, normal or critical, and anything else fails the " +
                    "run permanently rather than being retried: the input is the same on " +
                    "the next attempt, so three tries would be three identical failures and " +
                    "a minute of waiting for nothing.\n\n" +
                    "The rest are free-form and each has a defined empty meaning. No colour " +
                    "means the body goes through as plain text. No prefix means the title " +
                    "starts with rn:. No icon means the daemon draws its own. An expiry of " +
                    "0 means the daemon's own timeout rather than an instant dismissal.\n\n" +
                    "All of it is checked before the session bus is looked at, so a typo is " +
                    "reported as a typo rather than as a desktop problem.",
            },
            {
                name: "Find a desktop to draw on",
                lead: "No session bus means no screen — which is a skip, not a failure.",
                body:
                    "A notification daemon is reached over the session bus, and its address " +
                    "arrives in DBUS_SESSION_BUS_ADDRESS. If that variable is empty the run " +
                    "ends as skipped, naming the reason.\n\n" +
                    "Skipped rather than failed, deliberately. A machine with nobody logged " +
                    "in graphically has no screen to draw on, which is an ordinary state " +
                    "and not a fault; a red run every half hour on a headless box is noise " +
                    "that teaches people to ignore red runs.\n\n" +
                    "The variable is also the one widening in the launcher's environment " +
                    "seal. rn hands its Node child an explicitly constructed environment " +
                    "rather than inheriting yours, so this has to be passed through on " +
                    "purpose. If somebody is logged in and this step still skips, that pass- " +
                    "through is what to look at rather than the desktop.",
            },
            {
                name: "Find notify-send",
                lead: "Two absolute paths, checked directly — never a name resolved through PATH.",
                body:
                    "The program is looked for at /usr/bin/notify-send and /bin/notify-send, " +
                    "and the first one that exists is used. Nothing is resolved through " +
                    "PATH.\n\n" +
                    "That is the same rule the launcher follows with Node, for the same " +
                    "reason: rn grants its child a minimal PATH precisely so that what gets " +
                    "executed is not a question the environment answers, and a job that " +
                    "spawned notify-send by name would hand that decision back to whatever " +
                    "the environment happens to say.\n\n" +
                    "Neither path existing is a permanent failure. A missing program is " +
                    "still missing on the next attempt, so retrying would only delay the " +
                    "report. The fix is to install libnotify-bin, which is what the error " +
                    "names.",
            },
            {
                name: "Build the notification",
                lead: "Title and body from the run that triggered this one — or a test message when there is none.",
                body:
                    "This job is normally somebody else's handler, and the run that " +
                    "triggered it arrives as the cause: its job id, its outcome, its " +
                    "summary and its steps. The title is rn: <job> — <outcome>; the body is " +
                    "the summary one line per value, then the steps worth a glance.\n\n" +
                    "Which steps is a judgement made here: message steps become the sender " +
                    "and subject, link steps become the URL. Everything else is machinery, " +
                    "and a notification is not the place to read machinery. The body is cut " +
                    "at 800 characters — a notification is a glance, and a daemon handed " +
                    "four kilobytes either truncates it somewhere unhelpful or draws a " +
                    "panel across the screen.\n\n" +
                    "With no cause — you pressed Run now — it builds a test notification " +
                    "that says it is a test. One that arrives looking like real news and is " +
                    "not teaches you to distrust the next one.\n\n" +
                    "A colour wraps the body in Pango markup, and the body is escaped " +
                    "first, because it is mail somebody else composed. On this desktop the " +
                    "colour is inert — xfce4-notifyd renders bold and drops the foreground " +
                    "attribute, measured rather than assumed — which is why the title " +
                    "prefix exists: an emoji is a colour glyph drawn by the font, and no " +
                    "stylesheet can grey it out.",
            },
            {
                name: "Withhold the notification under DRY_RUN",
                lead: "The message is built, sized and titled; only the daemon is not called.",
                body:
                    "Everything above has already happened by the time dry run matters: the " +
                    "inputs were checked, the desktop was found, the message was built. " +
                    "What is withheld is one call to notify-send.\n\n" +
                    "The would-notify step carries the title and the size, so a disarmed " +
                    "install can be read to see what would have appeared. The run is " +
                    "recorded as skipped and changed: false.",
                reports:
                    "would-notify — the title, the body length, the urgency, and the colour " +
                    "if one was set.",
            },
            {
                name: "Draw it",
                lead: "execFile with an argument array, no shell, and a ceiling on a wedged daemon.",
                body:
                    "notify-send is run with its arguments as an array: the app name, the " +
                    "urgency, an expiry if one was asked for, an icon if one was named, " +
                    "then a literal -- and finally the title and body as positional " +
                    "arguments.\n\n" +
                    "execFile rather than exec, so there is no shell anywhere in the path. " +
                    "This matters more here than almost anywhere else in rn: the title and " +
                    "body are built from mail somebody else wrote, and a shell in that path " +
                    "would make a subject line a command. The -- is what keeps a body " +
                    "starting with a dash from being read as an option.\n\n" +
                    "The call is bounded at ten seconds. A wedged notification daemon must " +
                    "not hold this job in flight, because the job that triggered it is " +
                    "still registered as running too, and a restart queues behind both.\n\n" +
                    "A successful call returns changed: true — the one thing this job " +
                    "changes is what is on your screen.",
                reports:
                    "notified — the title, body length, urgency, icon and expiry actually " +
                    "used.",
            },
        ],
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const urgency = String(ctx.input.urgency ?? "normal").trim().toLowerCase();
        if (!["low", "normal", "critical"].includes(urgency)) {
            throw new PermanentFailure(
                `desktop-notify: "${urgency}" is not an urgency — low, normal or critical`,
                "the input is wrong, and it is the same input on the next attempt",
            );
        }

        const color = String(ctx.input.color ?? "").trim();
        const prefix = String(ctx.input.prefix ?? "");
        const icon = String(ctx.input.icon ?? "").trim();
        const expireSeconds = Math.max(0, Number(ctx.input.expireSeconds ?? 0));

        const bus = process.env["DBUS_SESSION_BUS_ADDRESS"];
        if (bus === undefined || bus === "") {
            return {
                summary: { delivered: false, reason: "no session bus" },
                changed: false,
                skipped:
                    "No DBUS_SESSION_BUS_ADDRESS, so there is no desktop session to notify. " +
                    "That is the ordinary state of a machine with nobody logged in " +
                    "graphically — nothing is wrong. If somebody is logged in and this still " +
                    "says so, the launcher is not passing the variable through: see " +
                    "launcher/src/main.rs.",
            };
        }

        const binary = findNotifySend();
        if (binary === undefined) {
            throw new PermanentFailure(
                `desktop-notify: no notify-send at ${CANDIDATES.join(" or ")}`,
                "a missing program is missing on the next attempt too",
            );
        }

        const { title: plainTitle, body: plain } = buildNotification(ctx.cause);
        const title = `${prefix}${plainTitle}`;

        // The span is kept for the desktops that honour it — dunst and mako
        // colour from it — and it is measurably inert on this one:
        // xfce4-notifyd renders <b> from the body and drops the foreground
        // attribute, which is why the title prefix exists and carries the
        // default. Escaped first either way: the body is somebody else's mail.
        const body =
            color === "" ? plain : `<span foreground="${color}">${escapeMarkup(plain)}</span>`;

        if (ctx.dryRun) {
            ctx.step("would-notify", { title, chars: body.length, urgency, color: color || "none" });
            return {
                summary: { delivered: false, chars: body.length, urgency },
                changed: false,
                skipped:
                    `DRY_RUN is on — nothing was shown. The notification is titled ` +
                    `"${title}" and is in the would-notify step above.`,
            };
        }

        await new Promise<void>((resolve, reject) => {
            // execFile, not exec: no shell, and the arguments reach the program
            // exactly as written. The body is mail somebody else composed.
            // Options before the `--`, which separates them from the two
            // positional arguments.
            const expiry =
                expireSeconds > 0 ? ["--expire-time", String(expireSeconds * 1000)] : [];
            const iconArg = icon === "" ? [] : ["--icon", icon];
            execFile(
                binary,
                ["--app-name=rn", `--urgency=${urgency}`, ...expiry, ...iconArg, "--", title, body],
                { timeout: TIMEOUT_MS },
                (err) => (err === null ? resolve() : reject(err)),
            );
        });

        ctx.step("notified", { title, chars: body.length, urgency, icon: icon || "none", expireSeconds });
        return {
            summary: { delivered: true, chars: body.length, urgency, icon: icon || "none", expireSeconds },
            changed: true,
        };
    },
};
