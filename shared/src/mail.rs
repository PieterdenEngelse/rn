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
