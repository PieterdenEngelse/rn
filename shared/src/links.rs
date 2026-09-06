//! Tracked links, and what came back to them.
//!
//! The shapes the Links page reads. Defined here for the reason everything else
//! in this crate is: `be` renames a field, `fe` stops compiling, rather than
//! `fe` rendering `undefined` in whichever panel reads it first.
//!
//! **What is deliberately not here.** No type in this module can carry a click
//! count without also carrying what the count is made of. `docs/link-tracking.md`
//! §5 is the reason: an arrival at `/t/<id>` cannot be attributed to a person,
//! because delivery-time scanners fetch every URL in a mail before anybody
//! reads it, and a forwarded link is clicked by somebody it was not minted for.
//! A page that showed `clicks: 4` and nothing else would be stating a fact it
//! does not have. So [`TrackedLink`] carries the method and the age of each
//! arrival alongside the total, and the page shows them together.

use crate::wire;

wire! {
    /// One arrival at a tracked link.
    ///
    /// The user-agent and the method are evidence rather than a filter. No
    /// browser navigates with `HEAD`, so a `HEAD` arrival is a link checker or
    /// a scanner and never a person — the strongest single signal in the store,
    /// and the reason it is kept rather than dropped at the door.
    #[serde(rename_all = "camelCase")]
    pub struct LinkClick {
        pub at: f64,
        pub user_agent: String,
        /// `GET` or `HEAD`.
        pub method: String,
        /// Milliseconds between the link being minted and this arrival.
        ///
        /// Carried rather than computed in the page because it is the number
        /// that makes a scanner visible: a click a second after the mail was
        /// sent is a machine, whatever its user-agent claims. It is not a
        /// filter — the row is shown either way — it is the column that lets a
        /// person distrust the total for themselves.
        pub after_mint_ms: f64,
    }
}

wire! {
    /// One link minted into one message.
    #[serde(rename_all = "camelCase")]
    pub struct TrackedLink {
        pub id: String,
        pub send_id: String,
        /// Absent for a per-send link — one id shared by everyone the mail went
        /// to, which answers *did this land* and cannot answer *who*.
        ///
        /// Also absent once identity has aged out: the recipient is dropped at
        /// `retentionDays` while the link goes on resolving forever, because a
        /// link in a mailbox may be clicked years later and a 404 there is a
        /// fault rather than lost analytics.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub recipient: Option<String>,
        /// Where it actually goes. The tracker reads this from its store and
        /// never from the request — see `be/src/tracker/server.ts`.
        pub url: String,
        pub minted_at: f64,
        /// Every arrival, oldest first. Not a count: see the module note.
        #[serde(default)]
        pub clicks: Vec<LinkClick>,
    }
}

wire! {
    /// One send, as the list page shows it.
    #[serde(rename_all = "camelCase")]
    pub struct TrackedSend {
        pub id: String,
        pub minted_at: f64,
        pub links: u32,
        pub clicks: u32,
        /// True when any link in this send names a recipient.
        ///
        /// A property of the send rather than a setting, because it is what the
        /// send actually did: a page that showed the current default would be
        /// describing what the *next* send will do while claiming to describe
        /// this one.
        pub identified: bool,
        /// How many distinct recipients this send minted links for. Zero for a
        /// per-send send, and zero again once identity has aged out — which the
        /// page must not report as "nobody", hence `identified` beside it.
        pub recipients: u32,
    }
}

wire! {
    /// GET /api/links.
    #[serde(rename_all = "camelCase")]
    pub struct LinksResponse {
        /// Newest first.
        #[serde(default)]
        pub sends: Vec<TrackedSend>,
        /// What actually appears in the mail. Shown because a tracker minting
        /// `http://127.0.0.1:3012/t/...` is configured but useless, and that is
        /// invisible from anywhere except the link itself.
        pub base_url: String,
        /// True when the base URL is this machine's own loopback — the default,
        /// and a link nobody else can follow.
        pub base_url_is_loopback: bool,
        pub retention_days: u32,
        /// Whether the tracker is actually bound. A link in a mailbox that
        /// finds nothing listening is a recipient looking at a browser error,
        /// so this is not the same kind of "degraded" as a poller being down.
        pub listening: bool,
        pub port: u32,
    }
}

wire! {
    /// GET /api/links/:sendId.
    #[serde(rename_all = "camelCase")]
    pub struct SendDetail {
        pub id: String,
        #[serde(default)]
        pub links: Vec<TrackedLink>,
    }
}
