//! Mail rules: which mailboxes rn watches, and which messages in them count.
//!
//! The shapes behind Config → Mail. Defined here for the reason everything in
//! this crate is: `be` renames a field and `fe` stops compiling, rather than a
//! rule silently losing its recipient filter somewhere between the two.
//!
//! ## Why a list rather than three settings
//!
//! The first version was three install-wide values — one mailbox, one sender
//! filter, one recipient filter — and they can express exactly one rule. "Tell
//! me when she writes to me" and "tell me when I write to her" are two, and the
//! settings could hold either, never both: the filters are ANDed, and a sender
//! filter naming her and a recipient filter naming her cannot both hold for one
//! message.
//!
//! A list fixes that by construction. Rules for the same mailbox are ORed —
//! any one matching is enough — while the fields *within* a rule are ANDed, so
//! `from: me, to: her` still means what it says. That is the smallest change
//! that makes both directions sayable, and it happens to be the shape a person
//! already has in mind when they say "when X mails Y".

use crate::wire;

wire! {
    /// One rule: a mailbox, and what counts as interesting in it.
    #[serde(rename_all = "camelCase")]
    pub struct MailRule {
        /// Stable id, minted by `be`. Appears in no URL a stranger can reach.
        pub id: String,
        /// The IMAP mailbox this rule watches — `INBOX`, `[Gmail]/Sent Mail`,
        /// a label. Two rules naming the same mailbox share one connection.
        pub mailbox: String,
        /// Addresses or domains the sender must match. Empty means any sender.
        #[serde(default)]
        pub from: String,
        /// Addresses or domains a `To` or `Cc` must match. Empty means any.
        ///
        /// The field that makes a sent mailbox worth watching: there the sender
        /// is always you, so `from` matches everything and only this
        /// distinguishes one message from another.
        #[serde(default)]
        pub to: String,
        /// Off keeps the rule and stops it doing anything — including holding
        /// its mailbox's connection open, when no other enabled rule names it.
        pub enabled: bool,
        /// What this rule is for, in the operator's words. Optional, and worth
        /// filling in: a list of address pairs is unreadable six months later.
        #[serde(default)]
        pub label: String,
    }
}

wire! {
    /// One watched mailbox and whether its connection is up.
    ///
    /// Reported rather than assumed, because a dropped IMAP connection is the
    /// quietest failure in this feature: mail simply stops arriving promptly,
    /// with nothing red anywhere.
    #[serde(rename_all = "camelCase")]
    pub struct WatchedMailbox {
        pub mailbox: String,
        pub watching: bool,
        /// Why it is not, when it is not.
        #[serde(default)]
        pub error: Option<String>,
        /// Runs this mailbox has started since the process began.
        pub triggered: u32,
    }
}

wire! {
    /// GET /api/mail-rules.
    #[serde(rename_all = "camelCase")]
    pub struct MailRulesResponse {
        #[serde(default)]
        pub rules: Vec<MailRule>,
        /// Whether the watcher is switched on at all. Rules exist and do
        /// nothing without it, which is worth saying on the page rather than
        /// leaving somebody to wonder why a correct-looking rule is silent.
        pub watching_enabled: bool,
        /// One entry per mailbox a connection is held for.
        #[serde(default)]
        pub watched: Vec<WatchedMailbox>,
        /// True when rules exist but the running process has not loaded them —
        /// the save-then-restart window, in which the page and the process
        /// disagree about what is being watched.
        pub needs_restart: bool,
    }
}

wire! {
    /// PUT and DELETE /api/mail-rules/:id.
    #[serde(rename_all = "camelCase")]
    pub struct MailRuleSaveResponse {
        pub ok: bool,
        /// What was wrong, in the order a person would fix it. Empty when ok.
        #[serde(default)]
        pub errors: Vec<String>,
        /// The rule as stored, with its id and any normalisation applied.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub rule: Option<MailRule>,
    }
}

wire! {
    /// One of the two servers the account talks to.
    ///
    /// Both ends are described by the same three facts, so they are one type
    /// rather than an `imapHost`/`smtpHost` pair of fields: the page draws the
    /// two boards from it and cannot accidentally render IMAP's port beside
    /// SMTP's host.
    #[serde(rename_all = "camelCase")]
    pub struct MailServer {
        pub host: String,
        pub port: u16,
        /// Whether this port is the implicit-TLS one — 993 for IMAP, 465 for
        /// SMTP. Derived from the port rather than configured, in `be`, by the
        /// same rule the two clients use: hardcoding `true` would have made the
        /// port setting a lie, and reporting it here from a second source would
        /// let the page disagree with the connection it describes.
        pub implicit_tls: bool,
    }
}

wire! {
    /// GET /api/mail-health. What each half of the mail account is doing.
    ///
    /// Two directions that share an account and nothing else. Reading is a
    /// held-open IMAP connection whose failure is silence; sending is an SMTP
    /// connection whose failure reaches somebody's inbox, or does not. They
    /// were one board on Config → Runtime and had no live surface at all —
    /// the watch state was rendered on a *config* page, which is where you go
    /// to change a thing rather than to see what it is doing.
    #[serde(rename_all = "camelCase")]
    pub struct MailHealthResponse {
        /// The address both directions authenticate as, and put in a `From`.
        pub user: String,
        /// Whether `gmailAppPassword` is set. Never the value — see
        /// `docs/token-sec.md`: a page reports that a secret exists and never
        /// what it is.
        pub credential_set: bool,

        pub imap: MailServer,
        /// How long a run's IMAP socket may sit silent, in ms. Not applied to
        /// the held-open watch connection — see the setting.
        pub imap_timeout_ms: f64,
        /// Whether the watcher is switched on at all.
        pub watching_enabled: bool,
        /// One entry per mailbox a connection is held for. Empty with watching
        /// on means no enabled rule names a mailbox.
        #[serde(default)]
        pub watched: Vec<WatchedMailbox>,
        /// Enabled rules, which is what decides the list above.
        pub rules_enabled: u32,
        /// Addresses arriving mail is narrowed to, as configured. Empty means
        /// the rules alone decide.
        #[serde(default)]
        pub allowed_senders: String,

        /// Addresses a `To` or `Cc` must match for arriving mail to count.
        /// Empty means any.
        ///
        /// An *inbound* filter, beside `allowed_senders` and applied in the
        /// same search — despite the name, which reads like a send guard and
        /// was misfiled as one here for exactly that reason. There is no
        /// install-wide list bounding who a send may go to; a send's
        /// recipients are the ones given to that run.
        #[serde(default)]
        pub allowed_recipients: String,

        pub smtp: MailServer,
        /// How long an SMTP socket may sit silent, in ms.
        pub smtp_timeout_ms: f64,
        /// Where replies are directed, or empty for the sending account.
        #[serde(default)]
        pub reply_to: String,
        /// The only addresses a send may go to. Empty refuses every send,
        /// which is the default and not the same as "no limit".
        ///
        /// The outbound guard, and deliberately not spelled like
        /// `allowed_recipients` above: that one is inbound, and the two were
        /// confused here once already.
        #[serde(default)]
        pub send_allowed_recipients: String,
        /// The name shown beside the address on outgoing mail. Empty sends the
        /// bare address, which is the default.
        #[serde(default)]
        pub from_name: String,
        /// Sends recorded in the link store. Zero is the ordinary state until
        /// the first send, and the page says so rather than showing a bare 0.
        pub sends: u32,
    }
}

wire! {
    /// One end's answer to "does this actually work", from POST /api/mail-test.
    ///
    /// A duration as well as a verdict, because the interesting failure here is
    /// not a refusal — that arrives with a reason — but a connection that takes
    /// twenty seconds to succeed. Neither client sets a timeout, so both
    /// inherit their library's, and nothing else on any page would show you
    /// that the server is answering slowly.
    #[serde(rename_all = "camelCase")]
    pub struct MailTestResult {
        pub ok: bool,
        /// How long the attempt took, whether it worked or not.
        pub ms: f64,
        /// Why it did not, when it did not. Redacted like every other message
        /// that leaves `be` — see `docs/token-sec.md`.
        #[serde(default)]
        pub error: Option<String>,
    }
}

wire! {
    /// POST /api/mail-test. Both ends, tried independently.
    ///
    /// Independently on purpose: they share an account, so the useful answer
    /// when a password is wrong is that *both* failed, and the useful answer
    /// when one host is unreachable is which one. Stopping at the first
    /// failure would report the second as untested and read as a consequence
    /// of the first.
    #[serde(rename_all = "camelCase")]
    pub struct MailTestResponse {
        pub imap: MailTestResult,
        pub smtp: MailTestResult,
    }
}
