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
    /// A reason a tracker base URL should not be minted into real mail.
    ///
    /// Every one of these is permanent in a way nothing else in rn is. A link
    /// lives in a mailbox for as long as the recipient keeps the message, so a
    /// base URL is not a setting that can be corrected later — it can only be
    /// corrected for mail not yet sent. That asymmetry is why this is a checked
    /// list rather than advice in a doc.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum BaseUrlProblem {
        /// Not a URL at all.
        Malformed,
        /// `http://`. A redirect a network can read and rewrite, and a scheme
        /// mail filters mark down on sight.
        Insecure,
        /// This machine's own address. Every link works perfectly here and is
        /// dead for every recipient — the failure that looks like success.
        Loopback,
        /// A bare address with no name. Unmovable if the address changes, and
        /// read as phishing by filters and by people.
        IpLiteral,
        /// An explicit non-default port. `docs/link-tracking.md` §3: a port
        /// inside an emailed URL reads as phishing to both filters and people.
        Port,
        /// A hostname under a suffix somebody else owns — `*.ts.net`, a quick
        /// tunnel, an ngrok name.
        ///
        /// The one that is hardest to see and worst to get wrong. It works, it
        /// looks fine, and the name is not yours: it changes if the machine is
        /// renamed or leaves the tailnet, and it cannot be pointed anywhere
        /// else afterwards. Every link already sent dies with it, in mail
        /// people kept. A domain you own is the only kind you can still
        /// redirect in five years.
        Borrowed,
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
        /// Everything wrong with the base URL, empty when it is fit to mint.
        ///
        /// A list rather than a worst-problem, because the default
        /// (`http://127.0.0.1:3012/t`) trips three at once and fixing one of
        /// them changes nothing a recipient would notice.
        #[serde(default)]
        pub base_url_problems: Vec<BaseUrlProblem>,
        /// Which of those the operator has accepted by configuration.
        ///
        /// Sent alongside rather than subtracted from `base_url_problems`,
        /// because a page that showed an accepted problem as no problem would
        /// be hiding the decision from the person who has to live with it.
        /// Something is still wrong with the URL; somebody has said they know.
        #[serde(default)]
        pub base_url_accepted: Vec<BaseUrlProblem>,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire spellings, pinned.
    ///
    /// `be` sends these strings and `fe` matches on them. A variant renamed
    /// without a `#[serde(rename)]` would leave the page unable to say what is
    /// wrong with a base URL, on the one screen whose job is to say so before
    /// a link is permanent.
    #[test]
    fn base_url_problems_keep_their_wire_spelling() {
        let one = |p: &BaseUrlProblem| serde_json::to_string(p).expect("serialises");

        assert_eq!(one(&BaseUrlProblem::Malformed), "\"malformed\"");
        assert_eq!(one(&BaseUrlProblem::Insecure), "\"insecure\"");
        assert_eq!(one(&BaseUrlProblem::Loopback), "\"loopback\"");
        assert_eq!(one(&BaseUrlProblem::IpLiteral), "\"ip-literal\"");
        assert_eq!(one(&BaseUrlProblem::Port), "\"port\"");
        assert_eq!(one(&BaseUrlProblem::Borrowed), "\"borrowed\"");
    }

    /// A response from a tracker with nothing wrong with it omits the list.
    #[test]
    fn a_clean_base_url_parses_without_the_problems_key() {
        let out: LinksResponse = serde_json::from_str(
            r#"{"sends":[],"baseUrl":"https://links.example.com/t","retentionDays":90,"listening":true,"port":3012}"#,
        )
        .expect("parses");
        assert!(out.base_url_problems.is_empty());
    }
}
