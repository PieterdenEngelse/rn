//! Monitor → Mail. What each half of the mail account is doing.
//!
//! One account, two protocols, and almost nothing shared between them. rn
//! reads over IMAP and sends over SMTP: different server, different port,
//! different client, different failure. The only thing both use is the address
//! and the password, which is why those sit in a board of their own beside the
//! two rather than being repeated in each.
//!
//! ## Why the two directions are drawn apart
//!
//! They were one board on Config → Runtime, nine settings deep, with
//! `imapPort` two rows from `smtpHost` and nothing saying they were opposite
//! directions. That is a reasonable way to file settings by topic and a poor
//! way to answer "is mail arriving" or "can this machine send", which are
//! separate questions with separate answers.
//!
//! Their failures are not alike either, and that is the stronger argument:
//!
//! - **Receiving fails silently.** A dropped IMAP connection means mail stops
//!   arriving promptly with nothing red anywhere — the `read-mail` schedule
//!   still runs, so the job history looks healthy while the watch is down.
//! - **Sending fails outward.** A run either reaches somebody's inbox or does
//!   not, and no revert reaches what did.
//!
//! ## Why this is under Monitor and the rules are under Config
//!
//! A rule is something you make; a connection is something that is happening.
//! Config → Mail keeps the rules and the per-rule state that belongs beside
//! them — "is this rule live" is a question about the rule. This page answers
//! the question about the two connections, which had no home at all: the watch
//! state was rendered only on a config page, which is where you go to change a
//! thing rather than to see what it is doing.

use crate::api::{
    fetch_jobs, fetch_mail_health, test_mail, JobsResponse, MailHealthResponse, MailServer,
    MailTestResponse, MailTestResult, WatchedMailbox,
};
use crate::app::Route;
use crate::components::{Board, InfoButton, Metric, Panel};
use crate::pages::monitor_jobs::{Catalogue, RunLog};
use crate::pages::MAIL_JOB_IDS;
use dioxus::prelude::*;
use dioxus_router::Link;

#[component]
pub fn MonitorMail() -> Element {
    let mut data = use_signal(|| Option::<Result<MailHealthResponse, String>>::None);

    use_future(move || async move {
        data.set(Some(fetch_mail_health().await));
    });

    let snapshot = data.read().clone();

    rsx! {
        div { class: "p-6 w-full space-y-4",

            div { class: "flex items-center gap-2",
                h1 { class: "text-xl text-white", "Mail" }
                InfoButton {
                    title: "The two halves of the mail account".to_string(),
                    what: "rn reads mail over IMAP and sends it over SMTP. Two protocols, two servers, two ports, two clients — and one account, which is the only thing they share: the same address authenticates both and the same gmailAppPassword credential answers for both.\n\nReading is a connection held open on a selected mailbox, which reports a message the moment it lands. Sending is a connection opened for one run and closed again.".to_string(),
                    why: "Because \"is mail working\" is two questions with two answers, and they fail in opposite ways.\n\nA dropped read connection is silent: mail simply stops arriving promptly, the half-hourly schedule keeps running, and the job history goes on looking healthy. A failed send is the reverse — loud, and already outside this machine. Anything that reaches a recipient cannot be taken back, and there is no install-wide list of addresses standing in front of it: the recipients of a send are the ones typed into that run, and DRY_RUN plus the credential are what stop a half-configured install from mailing anybody.".to_string(),
                    if_wrong: "If the password is missing both halves refuse before opening a connection, saying so plainly rather than failing at the protocol. If the address is empty the same.\n\nThe quiet failure to watch for is a watched mailbox that reports healthy and never fires — a connection to a mailbox nothing is delivered to looks exactly like a connection to a mailbox that is working. The mailboxes below show how many runs each has actually started, which is the number that distinguishes them.".to_string(),
                }
            }

            match snapshot {
                None => rsx! { p { class: "text-gray-400 text-sm", "Loading…" } },
                Some(Err(e)) => rsx! {
                    Panel { title: "The mail account could not be read".to_string(),
                        p { class: "text-gray-300 text-sm max-w-3xl", "{e}" }
                    }
                },
                Some(Ok(d)) => rsx! {
                    // All three in one wrapping row. The account was on a row
                    // of its own to say it belongs to both directions, and on
                    // a wide display that read as a gap rather than as a
                    // hierarchy — a short board alone with two thirds of the
                    // window empty beside it. Left to right says the same
                    // thing and uses the room.
                    div { class: "flex flex-wrap gap-4 items-start",
                        Account { data: d.clone() }
                        Receiving { data: d.clone() }
                        Sending { data: d.clone() }
                    }
                    ConnectionTest {}
                    Mailboxes { data: d.clone() }
                    MailJobs {}
                },
            }
        }
    }
}

/// The address and the password — the one thing both directions share.
///
/// Its own board rather than a row in each of the other two: repeating it
/// would say the two halves each have an account, and changing it would then
/// look like a change to one direction.
#[component]
fn Account(data: MailHealthResponse) -> Element {
    let user = if data.user.is_empty() { "not set".to_string() } else { data.user.clone() };
    let credential = if data.credential_set { "set" } else { "not set" };

    rsx! {
        Board { title: "Account".to_string(),
            Metric {
                label: "Address".to_string(),
                value: user,
                what: "The mailbox rn authenticates as, on both servers. It is also the From on anything the sending job puts out — RN_MAIL_USER on Config → Runtime, in the Mail account board.".to_string(),
                why: "One address for both directions is what makes the two halves halves of one thing rather than two unrelated integrations. It is also why the account is a board of its own rather than a row inside each of the other two — repeated, it would read as two accounts, and changing it would look like a change to one direction.".to_string(),
                if_wrong: "Empty and both mail jobs refuse before opening a connection, which is the good failure: it happens here rather than as an authentication error against somebody's server. A wrong address fails at authentication instead, which reads the same as a wrong password.".to_string(),
            }
            Metric {
                label: "Password".to_string(),
                value: credential.to_string(),
                what: "Whether the gmailAppPassword credential exists. Set it on Config → Jobs; it is stored outside the settings file and is never sent to this page.".to_string(),
                why: "This row says whether, and deliberately never what. A password on a screen is a broadcast rather than a read — it reaches a screenshot, a screen share and a browser's memory — so every panel in rn reports that a secret is set and none of them shows one. docs/token-sec.md is the argument in full.".to_string(),
                if_wrong: "Missing, and both directions stop before connecting rather than failing an authentication exchange. For a Google account this is an app password rather than the account password, and an ordinary password will authenticate against nothing however carefully it is typed.".to_string(),
            }
        }
    }
}

/// The IMAP half.
#[component]
fn Receiving(data: MailHealthResponse) -> Element {
    let watching = if data.watching_enabled { "on" } else { "off" };
    let senders = if data.allowed_senders.is_empty() {
        "any".to_string()
    } else {
        data.allowed_senders.clone()
    };
    let recipients = if data.allowed_recipients.is_empty() {
        "any".to_string()
    } else {
        data.allowed_recipients.clone()
    };

    rsx! {
        Board { title: "Receiving — IMAP".to_string(),
            Metric {
                label: "Server".to_string(),
                value: endpoint(&data.imap),
                what: "The IMAP server and port rn opens to read mail — RN_IMAP_HOST and RN_IMAP_PORT, in the Mail — receiving board on Config → Runtime.".to_string(),
                why: "Reading is the cheap half of link tracking: it needs no public endpoint, no tunnel and no unauthenticated stranger, unlike the outbound side. It is also the half that works today.".to_string(),
                if_wrong: "A wrong host fails the run visibly. The more interesting failure is a right host the process cannot reach, which under a restricted runtime looks like a connection error and is really a missing network grant — the job says which of the two it hit.".to_string(),
            }
            Metric {
                label: "Transport".to_string(),
                value: transport(&data.imap, 993),
                what: "Whether this port means TLS from the first byte. 993 is IMAP's implicit-TLS port, the way 465 is SMTP's, and rn derives the setting from the port rather than carrying a separate switch.".to_string(),
                why: "A separate switch would let the two disagree, and the disagreement that matters is the quiet one: a port changed to something plain with TLS still forced on fails to connect, and a port left at 993 with TLS off fails to negotiate. Deriving it means the port is the whole answer, and hardcoding it on would have made the port setting a lie.".to_string(),
                if_wrong: "Anything other than 993 here reports plain, which is correct for a server on a bench and wrong for anything reachable over a network you do not own. Mail is not the place to find out.".to_string(),
            }
            Metric {
                label: "Watching".to_string(),
                value: watching.to_string(),
                what: "Whether rn holds a connection open to notice mail as it lands, rather than only reading on the read-mail schedule.".to_string(),
                why: "Polling has a floor. A thirty-minute schedule reports a message up to thirty minutes after it arrived, and a held connection reports it within a couple of seconds — the difference between an automation that reacts and one that catches up.".to_string(),
                if_wrong: "Off, and the rules below still filter but nothing is prompt. The schedule is what guarantees mail is eventually read either way, which is the point: this is an improvement on the floor, never the only path.".to_string(),
            }
            Metric {
                label: "Enabled rules".to_string(),
                value: format!("{}", data.rules_enabled),
                what: "How many mail rules are switched on. Each names one mailbox and what counts as interesting in it; rules on the same mailbox share one connection.".to_string(),
                why: "It is what decides the mailboxes below. A rule switched off keeps its settings and stops holding its mailbox open, so this number and that list should agree — when they do not, the process has not been restarted since the rules changed.".to_string(),
                if_wrong: "Zero with watching on means nothing is held open at all, which looks identical to a connection problem from anywhere except this row.".to_string(),
            }
            Metric {
                label: "Recipient filter".to_string(),
                value: recipients,
                what: "Addresses a To or Cc must match for an arriving message to count — RN_MAIL_ALLOWED_RECIPIENTS, applied in the same server-side search as the sender filter. Both apply when both are set.".to_string(),
                why: "It is what makes watching a sent mailbox worth doing. There the sender is always you, so a sender filter matches everything and only this distinguishes one message from another — \"from me to her\" needs both halves.".to_string(),
                if_wrong: "Empty means any recipient. It is a filter on mail that arrives and has nothing to do with sending: nothing here bounds who a send may go to.\n\nSaving it and not restarting is the failure worth knowing. It takes effect at startup, so between the two the page says mail is narrowed and the process still has no filter — read-mail refuses to run rather than reading the whole mailbox while somebody believes otherwise.".to_string(),
            }
            Metric {
                label: "Sender filter".to_string(),
                value: senders,
                what: "Addresses or domains arriving mail is narrowed to before anything is reported — RN_MAIL_ALLOWED_SENDERS, applied in the server-side search rather than after fetching.".to_string(),
                why: "Narrowing on the server means mail from everybody else is never fetched, never scanned and never recorded. That is a privacy property rather than an optimisation: what is not read cannot end up in a run record.".to_string(),
                if_wrong: "Empty means any sender, and the rules alone decide. A domain matches as a suffix on @domain rather than as a substring, because notexample.com contains example.com and anybody can register it.".to_string(),
            }
        }
    }
}

/// The SMTP half.
#[component]
fn Sending(data: MailHealthResponse) -> Element {
    let from_name = if data.from_name.is_empty() {
        "none — the address alone".to_string()
    } else {
        data.from_name.clone()
    };
    let sends = if data.sends == 0 {
        "none yet".to_string()
    } else {
        format!("{}", data.sends)
    };

    rsx! {
        Board { title: "Sending — SMTP".to_string(),
            Metric {
                label: "Server".to_string(),
                value: endpoint(&data.smtp),
                what: "The SMTP server and port the sending job connects to — RN_SMTP_HOST and RN_SMTP_PORT, in the Mail — sending board on Config → Runtime.".to_string(),
                why: "This is the only thing in rn whose effect reaches a stranger. Everything else here either reads the world or writes inside this machine; a send puts an artefact in somebody else's hands, and no revert reaches it.".to_string(),
                if_wrong: "A wrong host fails the run before anything goes out, which is the harmless way to get this wrong. There is no harmless way to get the recipients wrong.".to_string(),
            }
            Metric {
                label: "Transport".to_string(),
                value: transport(&data.smtp, 465),
                what: "Whether this port means TLS from the first byte. 465 is SMTP's implicit-TLS port and is what rn connects with; the setting is derived from the port, exactly as the receiving half derives its own from 993.".to_string(),
                why: "Same argument as the other board, with more at stake: the message being carried is one you wrote to somebody, and the alternative to implicit TLS is a connection that starts in plaintext and upgrades if asked nicely.".to_string(),
                if_wrong: "Anything but 465 reports plain here. 587 with STARTTLS is a perfectly ordinary way to send mail and is not what this client does, so the port is not a free choice.".to_string(),
            }
            Metric {
                label: "From name".to_string(),
                value: from_name,
                what: "The name shown beside the address on mail this install sends — RN_MAIL_FROM_NAME, in the Mail — sending board on Config → Runtime. Empty sends the bare address.".to_string(),
                why: "It is the only thing on either board that a stranger ever sees. Everything else here decides whether a connection works; this decides what is in the From column of somebody's mail client, next to a subject line, before they have opened anything.".to_string(),
                if_wrong: "Nothing fails, and it changes nothing already sent — the same one-way property the tracker's base URL has, for the same reason. The address beside it is still the account that authenticated, which is what a receiving server checks.".to_string(),
            }
            Metric {
                label: "Sends recorded".to_string(),
                value: sends,
                what: "How many sends the link store holds. A send is one message to one list; the links minted for it, and the arrivals at those links, are on Monitor → Links.".to_string(),
                why: "It is the sending half's only evidence of ever having worked. The receiving half has watched mailboxes and a run history; until a send happens this side has nothing but configuration, and configuration that has never been exercised is not the same as configuration that works.".to_string(),
                if_wrong: "None yet is the ordinary state and not a problem to fix. It does mean everything on this board is untested on this machine, including the parts that look settled.".to_string(),
            }
        }
    }
}

/// The two jobs, and what they have been doing.
///
/// The cards are here rather than on Monitor → Jobs because a card is about
/// one job and this is where that job's connection is: the Run button sits
/// beside the mailbox the run will read, and the schedule beside the watch
/// that decides whether the schedule is what you are waiting on.
///
/// The run log is a *second view*, not a move. The one on Monitor → Jobs still
/// holds every run, which is what makes it evidence — a chronological list has
/// no shape to say what is missing from it, and `caused_by` renders as "change
/// of read-mail" on the `notify-all` rows there, naming a run that has to be
/// findable in the same list. This one asks the same store the same question
/// with the scope already set.
#[component]
fn MailJobs() -> Element {
    let mut jobs = use_resource(fetch_jobs);
    let ids: Vec<String> = MAIL_JOB_IDS.iter().map(|s| (*s).to_string()).collect();

    rsx! {
        match &*jobs.read_unchecked() {
            Some(Ok(j)) => {
                let j: JobsResponse = j.clone();
                let mail_catalogue: Vec<_> = j
                    .catalogue
                    .iter()
                    .filter(|c| MAIL_JOB_IDS.contains(&c.id.as_str()))
                    .cloned()
                    .collect();
                rsx! {
                    Panel {
                        title: "Jobs".to_string(),
                        subtitle: Some("the two that use the connections above".to_string()),
                        Catalogue {
                            jobs: j.clone(),
                            mail_only: true,
                            on_ran: move |_| jobs.restart(),
                        }
                    }
                    Panel {
                        title: "Recent runs".to_string(),
                        subtitle: Some("read-mail and send-mail only".to_string()),
                        info: Some(rsx! {
                            InfoButton {
                                title: "This log and the one on Monitor → Jobs".to_string(),
                                what: "The same run record, asked with the scope already set to these two jobs. Every run here is also in Recent runs on Monitor → Jobs; nothing was moved out of it.\n\nThe filters work as they do there. \"Any mail job\" means these two rather than the whole record, and picking one narrows to it.".to_string(),
                                why: "Because the question this page answers is about mail, and setting the same filter by hand every time is how a person stops looking. The full log stays whole because it is the only thing that can answer \"what did this machine do\" — a chronological list has no shape to show what is missing from it, so a gap there would be invisible in a way a missing card is not.".to_string(),
                                if_wrong: "It asks for the last 25 of each job and keeps the newest 25 of the result, which is exactly what one query for both would return: a run in the most recent 25 of the pair is necessarily in the most recent 25 of its own job.\n\nThe counts under the filters read the same way as elsewhere — how many matched, out of how much record is kept at all. The kept figure is the whole store's, not these two jobs' share of it.".to_string(),
                            }
                        }),
                        RunLog { catalogue: mail_catalogue, only: ids.clone() }
                    }
                }
            }
            Some(Err(e)) => rsx! {
                Panel { title: "Jobs".to_string(),
                    p { class: "text-red-400 text-sm", "Backend unreachable" }
                    p { class: "text-gray-300 text-sm mt-1", "{e}" }
                }
            },
            None => rsx! {
                Panel { title: "Jobs".to_string(),
                    p { class: "text-gray-400 text-sm", "Loading…" }
                }
            },
        }
    }
}

/// Prove both connections, without sending anything or reading anything.
///
/// The gap this fills is the one the sending board makes obvious: with no send
/// on record, every value there is configuration that has never been
/// exercised, and the only way to learn whether it works was to mail a person.
/// Both clients can answer without that — IMAP by connecting and leaving,
/// nodemailer by `verify()`, which authenticates and quits.
///
/// Deliberately a button and not something the page does on load. It
/// authenticates against somebody else's server, and a page that did it every
/// time it was opened would be a login attempt per refresh.
#[component]
fn ConnectionTest() -> Element {
    let mut result = use_signal(|| Option::<Result<MailTestResponse, String>>::None);
    let mut running = use_signal(|| false);

    rsx! {
        div { class: "flex items-baseline gap-3 flex-wrap",
            button {
                class: "cursor-pointer text-xs",
                style: "color: #22d3ee;",
                disabled: running(),
                onclick: move |_| {
                    running.set(true);
                    result.set(None);
                    spawn(async move {
                        result.set(Some(test_mail().await));
                        running.set(false);
                    });
                },
                if running() { "Testing…" } else { "Test both connections" }
            }
            InfoButton {
                title: "Testing the connections".to_string(),
                what: "Opens each connection, completes TLS, authenticates with the account above, and closes it again. Nothing is sent, no mailbox is selected and no message is fetched — so nothing can be marked as read, which is the claim the reading half rests on.\n\nThe two are tried independently rather than one after the other, so a wrong password reports as both failing and an unreachable host reports as one.".to_string(),
                why: "Because until a send has happened, the sending board is configuration nobody has exercised — and the only other way to find out whether it works is to mail somebody, which is the one thing here that cannot be taken back.\n\nThe duration is reported for a reason of its own: neither client sets a timeout, so both inherit their library's — five minutes of socket silence for IMAP, ten for SMTP. A connection that succeeds slowly is invisible everywhere else on this page.".to_string(),
                if_wrong: "A refusal comes back with the server's own reason, redacted the way every message that leaves rn is. \"Invalid credentials\" against a Google account usually means an ordinary password where an app password is wanted.\n\nSuccess proves the account, the host, the port and TLS. It does not prove a mailbox exists, that a filter matches anything, or that mail you send will be accepted by the recipient's server — those fail later and elsewhere.".to_string(),
            }
            match result.read().as_ref() {
                Some(Ok(r)) => rsx! {
                    span { class: if r.imap.ok { "text-green-400 text-xs" } else { "text-amber-400 text-xs" },
                        "IMAP {outcome_label(&r.imap)}"
                    }
                    span { class: if r.smtp.ok { "text-green-400 text-xs" } else { "text-amber-400 text-xs" },
                        "SMTP {outcome_label(&r.smtp)}"
                    }
                },
                Some(Err(e)) => rsx! { span { class: "text-red-400 text-xs", "{e}" } },
                None => rsx! {},
            }
        }
    }
}

/// One end's verdict, with the number that matters either way.
///
/// The duration is kept on success as well as failure: a connection that works
/// in eight seconds is the case nothing else on the page can show, and the one
/// that turns into a job sitting under a five- or ten-minute library timeout.
fn outcome_label(r: &MailTestResult) -> String {
    match r.error.as_ref() {
        None if r.ok => format!("ok in {}ms", r.ms as i64),
        Some(e) => format!("failed after {}ms — {e}", r.ms as i64),
        None => format!("failed after {}ms", r.ms as i64),
    }
}

/// One row per mailbox a connection is held for.
///
/// A list rather than a count, because the failure this page exists for is
/// per-mailbox: one watch dropping while the others stay up looks, from every
/// other angle, exactly like nobody having written to that mailbox.
#[component]
fn Mailboxes(data: MailHealthResponse) -> Element {
    rsx! {
        Panel { title: "Watched mailboxes".to_string(),
            if !data.watching_enabled {
                p { class: "text-gray-300 text-sm max-w-3xl",
                    "Watching is off, so no connection is held. Mail is still read on the read-mail schedule, up to thirty minutes after it arrives. The switch is \"Read mail the moment it arrives\" in the Mail — receiving board on Config → Runtime."
                }
            } else if data.watched.is_empty() {
                p { class: "text-gray-300 text-sm max-w-3xl",
                    "Watching is on and nothing is being watched: no enabled rule names a mailbox. Rules are made on Config → Mail, and they take effect at restart."
                }
            } else {
                div { class: "space-y-1",
                    for m in data.watched.iter() {
                        MailboxRow { key: "{m.mailbox}", mailbox: m.clone() }
                    }
                }
            }
            p { class: "text-gray-400 text-xs mt-3 max-w-3xl",
                "Rules are edited on "
                Link { to: Route::ConfigMail {}, class: "text-blue-400 hover:text-blue-300", "Config → Mail" }
                ", and what the sending half produced is on "
                Link { to: Route::MonitorLinks {}, class: "text-blue-400 hover:text-blue-300", "Monitor → Links" }
                "."
            }
        }
    }
}

#[component]
fn MailboxRow(mailbox: WatchedMailbox) -> Element {
    rsx! {
        div { class: "flex items-baseline gap-3 flex-wrap",
            code { class: "text-gray-200 text-xs w-72 shrink-0", "{mailbox.mailbox}" }
            span {
                class: if mailbox.watching { "text-green-400 text-xs w-20 shrink-0" } else { "text-amber-400 text-xs w-20 shrink-0" },
                if mailbox.watching { "connected" } else { "down" }
            }
            // Runs started, not messages seen. A mailbox that is connected and
            // has started nothing is the quiet failure this page is for: it
            // looks identical to one that is working and has had no mail.
            span { class: "text-gray-400 text-xs shrink-0",
                "{mailbox.triggered} run(s) started"
            }
            if let Some(err) = mailbox.error.as_ref() {
                span { class: "text-gray-300 text-xs", "{err}" }
            }
        }
    }
}

/// `host:port`, the way both clients are configured.
///
/// A free function rather than a method: `MailServer` is defined in `shared/`
/// and `fe` cannot write an `impl` for a foreign type.
fn endpoint(s: &MailServer) -> String {
    format!("{}:{}", s.host, s.port)
}

/// What the port means for the connection, and the port that would mean TLS.
///
/// Names the implicit-TLS port when it is not the one in use, because "plain"
/// on its own leaves a reader to look up which number they wanted.
fn transport(s: &MailServer, tls_port: u16) -> String {
    if s.implicit_tls {
        "TLS from the first byte".to_string()
    } else {
        format!("plain — {tls_port} is the TLS port")
    }
}
