//! The six families a provider's webhook event names fall into, and the rule
//! that sorts a name into one of them.
//!
//! One copy, read by three places: the boards on Config → Webhooks, the
//! tallies on Monitor → Webhooks, and the Groups tab on Config → Connection's
//! Webhooks panel. The table began in that tab as a literal of its own; with a
//! second and third reader it would have become three tables that disagree
//! about what a lifecycle event is.
//!
//! **The families are the providers' convention, not rn's.** No provider is
//! obliged to follow them and the names differ between every one of them.
//! Nothing here filters or routes on a family — a delivery reaches its job
//! whatever it is called. What the sorter is for is reading: forty deliveries
//! sorted into six families say what a provider is actually sending, and
//! forty event names in a list do not.
//!
//! The table and the sorter are not in `shared/`, because neither crosses a
//! process boundary: the backend records the provider's name verbatim, and
//! which family it belongs to is a question the page asks of it. What does
//! cross is `WebhookFamily` — the family a webhook was *made for* on one of the
//! boards, stored with its definition — and that one is shared.

use crate::api::{WebhookFamily, WebhookKind};

/// One family: what it means, what it asks of a job, and the words that put a
/// name in it.
#[derive(Clone, PartialEq, Debug)]
pub struct EventFamily {
    /// The stored spelling, for a webhook made on this family's board.
    pub id: WebhookFamily,
    pub name: &'static str,
    /// As they appear in the table the families come from, `;`-separated.
    pub examples: &'static str,
    pub meaning: &'static str,
    /// The words that sort a name here. Shown on the board, because a rule
    /// the reader cannot see is a rule they cannot argue with.
    pub words: &'static [&'static str],
    /// How the words are matched: anywhere in the name, or as its verb.
    pub by_subject: bool,
    /// One line: what a job receiving these has to be careful of.
    pub asks: &'static str,
    /// The kind a webhook for this family starts as when made on its board,
    /// and why. A starting point — the form still offers all three.
    pub kind: WebhookKind,
    pub kind_why: &'static str,
    /// The job a webhook made on this board starts with selected, when one
    /// job is the obvious answer for the whole family. `None` is most of them:
    /// what a create event should do is yours to write, and pre-selecting the
    /// first job in the list would be a guess dressed as advice.
    pub job: Option<&'static str>,
    pub what: &'static str,
    pub why: &'static str,
    pub if_wrong: &'static str,
}

pub const CREATE: usize = 0;
pub const UPDATE: usize = 1;
pub const DELETE: usize = 2;
pub const LIFECYCLE: usize = 3;
pub const SECURITY: usize = 4;
pub const SYSTEM: usize = 5;

/// A `static` rather than a `const`, so [`family`] can hand out a reference
/// that lives as long as the program instead of one to a temporary copy.
pub static FAMILIES: [EventFamily; 6] = [
    EventFamily {
        id: WebhookFamily::Create,
        name: "Create events",
        examples: "payment.created; order.created; user.registered",
        meaning: "Something new was created",
        // "opened" was here and moved to Lifecycle: in a name it is nearly always
        // a stage — an email read, a Stripe review that will later close.
        words: &[
            // brought into existence
            "created", "create", "creation", "new", "generated", "provisioned",
            // somebody arrived
            "registered", "signup", "join", "joined", "subscribe", "subscribed", "enrolled",
            // added to a collection
            "added", "add", "inserted", "insert", "appended", "uploaded", "imported",
            "submission", "booked",
            // made from something that already existed
            "copy", "copied", "duplicated", "cloned", "fork", "forked",
        ],
        by_subject: false,
        asks: "safe to run twice — a retried delivery is the same order again",
        kind: WebhookKind::DataPayload,
        kind_why: "the new record is in the body — there is nothing to go back for",
        job: None,
        what: "A provider telling you a record now exists that did not before — a payment, an \
               order, an account. The verb is usually the last word of the name: \
               `payment.created`, `user.registered`. GitHub is the exception worth knowing: its \
               event header says just `create`, with no noun, and means a branch or tag; its \
               `fork` lands here too. HubSpot uses the noun instead of the verb — \
               `contact.creation` — and Webflow's `form_submission` is a new record by another \
               name.",
        why: "Create deliveries are the ones a retry hurts. A provider waits a few seconds for a \
              2xx and sends again if none comes, so the same order can arrive twice. rn answers \
              202 before the job runs, which keeps retries rare, and the replay log refuses a \
              delivery id it has already seen — but only when the provider sends one and the \
              hook names the header it arrives in. A job that makes something of its own in \
              response (a row, an email, a ticket) should key it on the provider's id, so a \
              second delivery finds the work already done.",
        if_wrong: "Two of whatever the job makes: two welcome emails, two rows. Monitor → \
                   Webhooks shows the same event name twice a few seconds apart; the delivery id \
                   on each run in Monitor → Jobs says which case it was. The same id twice means \
                   the replay log never saw it — no delivery header configured. Two different \
                   ids means the provider really did send two.",
    },
    EventFamily {
        id: WebhookFamily::Update,
        name: "Update events",
        examples: "invoice.updated; subscription.changed; order.status.updated",
        meaning: "Something changed",
        words: &[
            // the content itself
            "updated", "update", "changed", "change", "edited", "edit", "modified", "revised",
            "amended", "patched", "adjusted", "corrected",
            // where it sits or what it is called
            "renamed", "moved", "move", "transferred", "reordered", "relocated",
            // swapped for something else
            "replaced", "overwritten", "upgraded", "downgraded", "migrated", "swapped",
            // two records made one — HubSpot's `contact.merge`. A pull request is
            // "merged", which is a stage and stays under Lifecycle.
            "merge",
            // what it is attached to
            "attached", "detached", "linked", "unlinked", "connect", "connected", "disconnect",
            "disconnected", "associated", "tagged", "untagged", "labeled", "unlabeled",
            "labelled", "unlabelled",
            // brought back in line with somewhere else
            "synchronize", "synchronized", "synchronised", "synced", "refreshed", "reconciled",
            // settings and amounts
            "configured", "reconfigured", "toggled", "increased", "decreased", "incremented",
            "decremented",
        ],
        by_subject: false,
        asks: "act on the current state — two edits can arrive in either order",
        kind: WebhookKind::Notification,
        kind_why: "rn fetches the record when the delivery lands, so an older edit arriving late cannot win",
        job: None,
        what: "Something that already existed changed: an invoice's amount, a subscription's \
               plan, an order's status. Usually `<noun>.updated` or `<noun>.changed`; some \
               providers name the field as well, as in `order.status.updated`, which is why the \
               last verb in the name decides rather than the first word. HubSpot and Trello \
               run the words together — `contact.propertyChange`, `moveCardToBoard` — and the \
               sorter splits them at each capital, so those read as change and move.",
        why: "Updates come in bursts, and two edits a second apart can be delivered in either \
              order — retries make that worse, not better. A job that writes what the delivery \
              says can end on the older value. That is the case for the Notification kind on \
              Config → Jobs: rn fetches the record by id when the delivery lands, so the job \
              acts on what is true now rather than on what was true when the event was sent.",
        if_wrong: "A value that drifts back to an earlier state for no visible reason. Compare the \
                   order of the runs on Monitor → Webhooks with the order of edits at the \
                   provider; if they differ, the hook wants to fetch first.",
    },
    EventFamily {
        id: WebhookFamily::Delete,
        name: "Delete events",
        examples: "customer.deleted; file.removed",
        meaning: "Something was removed",
        words: &["deleted", "delete", "removed", "remove", "destroyed", "destroy", "purged", "erased"],
        by_subject: false,
        asks: "deleting what is already gone is a success, not an error",
        kind: WebhookKind::DataPayload,
        kind_why: "a lookup would get a 404 for a deleted record, and the delivery would be dropped",
        job: None,
        what: "A record is gone at the provider: `customer.deleted`, `file.removed`. The body \
               often carries only the id, since there is nothing left to describe.",
        why: "The one family where fetching first fails by design. A Notification hook looks the \
              record up by id, the provider answers 404 because it was deleted, and rn counts \
              the delivery as dropped — lookup failed, no run started. Delete events want a \
              Data payload hook, or a job that expects the record to be missing. They are \
              retried like creates, so a second delivery must find nothing to do and say so \
              rather than fail.",
        if_wrong: "Deletions that never reach a job: the hook's dropped count climbs on Config → \
                   Jobs while no run appears here. Or a job that fails on every retry because \
                   what it removes was removed by the first delivery.",
    },
    EventFamily {
        id: WebhookFamily::Lifecycle,
        name: "Lifecycle events",
        examples: "payment.succeeded; payment.failed; shipment.delivered",
        meaning: "Resource moved through a stage",
        words: &[
            // under way, or waiting on someone
            "started", "start", "queued", "pending", "processing", "requested", "submitted",
            "initiated", "scheduled", "requires", "required",
            // how it ended
            "succeeded", "success", "failed", "failure", "completed", "complete", "finished",
            "done", "ended", "end", "cancelled", "canceled", "status",
            // money moving
            "paid", "captured", "refunded", "authorized", "authorised", "funded", "settled",
            "available", "finalized", "voided", "uncollectible", "reversed", "denied",
            "declined",
            // goods and messages moving
            "shipped", "dispatched", "delivered", "returned", "fulfilled", "sent",
            // somebody decided
            "approved", "rejected", "accepted", "confirmed", "signed", "verified", "reviewed",
            "escalated", "assigned", "unassigned", "resolved",
            // switched on, off, or put away
            "activated", "deactivated", "enabled", "disabled", "paused", "resumed", "suspended",
            "opened", "closed", "reopened", "archived", "unarchived", "restored",
            // made public
            "published", "unpublished", "released", "deployed", "merged",
            // the calendar
            "expired", "expiring", "renewed", "due", "overdue", "upcoming",
        ],
        by_subject: false,
        asks: "one transition can arrive as two events — pick one to act on",
        kind: WebhookKind::DataPayload,
        kind_why: "the stage is in the name and the resource in the body",
        job: None,
        what: "A resource moved to a new stage: a payment succeeded or failed, a shipment was \
               delivered, a trial expired. The noun stays the same across a whole family of \
               events and the verb names the stage — which is why this family has the longest \
               word list here, and why it is the one most likely to miss a provider's own verb.",
        why: "The most useful family to automate on, because each stage is a decision: send the \
              receipt, chase the failed card, close the ticket. It is also where one provider \
              sends several events for one transition — `invoice.paid` and \
              `invoice.payment_succeeded` for the same payment — so a job listening to both runs \
              twice for one thing.",
        if_wrong: "Two runs per transition, or a stage that never fires because the provider \
                   spells it differently from the documentation. The names on Monitor → Webhooks \
                   are what actually arrived; read them there.",
    },
    EventFamily {
        id: WebhookFamily::Security,
        name: "Security events",
        examples: "login.attempt; password.changed; api.key.revoked",
        meaning: "Security-related action occurred",
        // Grouped by what the word is evidence of. Some words that look like
        // they belong are left out on purpose, because they appear in other
        // families' names too: "suspended" (a subscription can be suspended
        // for billing), "authorization" (a card payment is authorised),
        // "block" (Notion's content blocks), "team" and "organization"
        // (GitHub sends both when a name or description is edited),
        // "session" (Stripe's `checkout.session.completed` is a payment) and
        // "member" (Ghost's and Discord's members are subscribers).
        words: &[
            // signing in
            "login", "logout", "signin", "authentication",
            // the secrets a person holds
            "password", "passwd", "mfa", "2fa", "otp", "totp", "webauthn", "passkey",
            // the secrets software holds
            "token", "tokens", "key", "keys", "secret", "credential", "credentials", "auth",
            "oauth",
            // who may do what
            "permission", "permissions", "role", "roles", "sso", "invite", "invited",
            "invitation",
            // an account locked or banned
            "lockout", "locked", "lock", "unlock", "unlocked", "ban", "banned",
            // the provider suspects something
            "security", "suspicious", "fraud", "breach", "compromised", "leaked",
            // the code or its dependencies are unsafe
            "vulnerability", "vulnerabilities", "advisory", "dependabot", "scanning", "cve",
        ],
        by_subject: true,
        asks: "worth a person seeing now — a notifier can be its job",
        kind: WebhookKind::DataPayload,
        kind_why: "act on what arrived at once — a lookup is a second call that can fail at the worst moment",
        job: Some("desktop-notify"),
        what: "Something happened to access itself: a login attempt, a password change, an API \
               key revoked. Recognised by the subject rather than the verb, and checked before \
               any verb, so `password.changed` is here rather than under Update and \
               `api.key.created` rather than under Create.",
        why: "The events worth acting on quickly and worth a person seeing, so a hook made on \
              this board starts with desktop-notify as its job: the delivery pops up on this \
              screen, titled with the event name and the hook it arrived on — `rn: \
              api.key.revoked — via github-security`. It needs no account, which is why it is \
              the default; notify-mail and notify reach you away from the desk, and notify-all \
              tries every one. What the message holds is what the listener recorded — event, \
              hook, delivery id — and never the delivery's body, which is the provider's data \
              and would leave the machine with mail. A job that wants a fact from the body in \
              the message reads it, reports it, and names a notifier as its on-change handler. \
              These are also where a forged delivery would do the most harm, which is why every \
              hook on the listener checks a signature or token and none accepts an unsigned \
              delivery.",
        if_wrong: "A revoked key a job goes on using until it fails, or a login from somewhere \
                   unexpected that nobody hears about. A notification that arrives saying \
                   \"webhook delivery\" instead of an event name means the hook reads the wrong \
                   header for this provider — set its event header. A security event under \
                   Unsorted on Monitor → Webhooks means its name uses a word this list does not \
                   have.",
    },
    EventFamily {
        id: WebhookFamily::System,
        name: "System events",
        examples: "server.alert; quota.exceeded; rate_limit.hit",
        meaning: "System behaviour or internal alert",
        // Grouped like the security list. Left out on purpose, because other
        // families use them: "status" (the lifecycle verb in
        // `deployment_status`), "limit" (spending and credit limits are
        // billing), "unavailable" (a product out of stock), "recovery" (account
        // recovery is a security event), "test" and "app" (Slack's
        // `app_mention` is a message, not the app reporting on itself).
        words: &[
            // the provider's own machinery
            "server", "system", "health", "healthcheck", "heartbeat", "ping", "uptime",
            // limits on your account
            "quota", "rate", "ratelimit", "throttle", "throttled", "throttling", "exceeded",
            "capacity",
            // the provider having a bad day
            "incident", "outage", "downtime", "disruption", "maintenance", "degraded",
            // errors and the monitoring that reports them
            "error", "errors", "exception", "crash", "crashed", "metric", "metrics", "monitor",
            "anomaly", "threshold", "latency", "alert",
            // the webhook and the integration themselves
            "webhook", "endpoint", "meta", "installation", "uninstalled",
        ],
        by_subject: true,
        asks: "high volume — filter at the provider, not inside the job",
        kind: WebhookKind::DataPayload,
        kind_why: "high volume — a lookup per delivery doubles the cost of every burst",
        job: None,
        what: "The provider talking about itself or about your account's limits: a server \
               raising an alarm, a quota exceeded, a rate limit hit. Recognised by words like \
               server, quota and rate anywhere in the name. GitHub's `ping`, sent once when a \
               hook is made, lands here too, and so does its `meta`, sent when the hook itself \
               is deleted — the one delivery that says no more will follow.",
        why: "The high-volume family. A rate-limit event can fire every few seconds for as long \
              as the condition lasts, and each delivery is a run and a record in a history that \
              keeps a fixed number of runs across every job — so a burst of these pushes other \
              jobs' records out. Most providers let you choose which events a hook is sent; \
              that is the place to filter, before a delivery costs anything here.",
        if_wrong: "One system event dominating the counts on Monitor → Webhooks, and the run \
                   history for unrelated jobs going short, because it is their records the burst \
                   evicts.",
    },
];

/// The row for a stored family.
pub fn family(id: &WebhookFamily) -> &'static EventFamily {
    FAMILIES
        .iter()
        .find(|f| &f.id == id)
        .expect("every WebhookFamily has a row — the test below says so")
}

/// Which family a name fell into, and the word that put it there.
#[derive(Clone, PartialEq, Debug)]
pub struct Sorted {
    pub family: usize,
    pub word: String,
}

/// The words of an event name, lowercased: `rate_limit.hit` is
/// `["rate", "limit", "hit"]`, and `contact.propertyChange` is
/// `["contact", "property", "change"]`.
///
/// A capital following a lowercase letter or digit starts a word, for the
/// providers that write names in camelCase. A capital following a capital does
/// not, so PayPal's `PAYMENT.CAPTURE.DENIED` stays three words and not nineteen.
fn words(event: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut prev: Option<char> = None;
    for c in event.chars() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else {
            let camel_hump = c.is_ascii_uppercase()
                && prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit());
            if camel_hump && !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            word.push(c.to_ascii_lowercase());
        }
        prev = Some(c);
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

/// Sort one event name.
///
/// Two passes, and the order is the rule. **Subject first**: a name with a
/// security or system word anywhere in it belongs to that family whatever its
/// verb, because `password.changed` is a security event that happens to be an
/// update, not the other way round. **Then the verb**, read from the end of the
/// name backwards, so `order.status.updated` is an update and not a lifecycle
/// event on the strength of `status`.
///
/// `None` for a name with no word the lists know — GitHub's `push` and
/// `pull_request` among them, because GitHub puts the verb in the body's
/// `action` field and rn records the header, not the body.
pub fn sort(event: &str) -> Option<Sorted> {
    let ws = words(event);
    for family in [SECURITY, SYSTEM] {
        if let Some(w) = ws.iter().find(|w| FAMILIES[family].words.contains(&w.as_str())) {
            return Some(Sorted { family, word: w.clone() });
        }
    }
    for w in ws.iter().rev() {
        for family in [DELETE, CREATE, UPDATE, LIFECYCLE] {
            if FAMILIES[family].words.contains(&w.as_str()) {
                return Some(Sorted { family, word: w.clone() });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_into(event: &str) -> Option<usize> {
        sort(event).map(|s| s.family)
    }

    /// Every example the table quotes lands in the row that quotes it —
    /// otherwise the boards would contradict their own examples.
    #[test]
    fn every_example_sorts_into_its_own_family() {
        for (i, f) in FAMILIES.iter().enumerate() {
            for example in f.examples.split(';').map(str::trim) {
                assert_eq!(sorted_into(example), Some(i), "{example} should be {}", f.name);
            }
        }
    }

    /// Index order is what `sort` returns, and the stored id is what a
    /// webhook carries; the two must name the same row.
    #[test]
    fn every_family_has_its_own_row_in_order() {
        let ids = [
            WebhookFamily::Create,
            WebhookFamily::Update,
            WebhookFamily::Delete,
            WebhookFamily::Lifecycle,
            WebhookFamily::Security,
            WebhookFamily::System,
        ];
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(&FAMILIES[i].id, id);
            assert_eq!(family(id).name, FAMILIES[i].name);
        }
    }

    #[test]
    fn subject_outranks_verb() {
        assert_eq!(sorted_into("api.key.created"), Some(SECURITY));
        assert_eq!(sorted_into("password.changed"), Some(SECURITY));
    }

    /// Real provider names that sorted as Unsorted, or as System on the strength
    /// of their last word, before the security list covered passwords by
    /// field name, locking and vulnerability reports.
    #[test]
    fn provider_security_names() {
        for event in [
            "user.account.update_password",      // Okta
            "user.account.lock",                 // Okta
            "deploy_key",                        // GitHub
            "secret_scanning_alert",             // GitHub
            "dependabot_alert",                  // GitHub
            "code_scanning_alert",               // GitHub
            "repository_vulnerability_alert",    // GitHub
            "security_advisory",                 // GitHub
            "radar.early_fraud_warning.created", // Stripe
        ] {
            assert_eq!(sorted_into(event), Some(SECURITY), "{event}");
        }
    }

    /// The words left out of the security list on purpose stay where their
    /// verb puts them, and a system event with no security word is still one.
    #[test]
    fn near_misses_stay_out_of_security() {
        assert_eq!(sorted_into("server.alert"), Some(SYSTEM));
        assert_eq!(sorted_into("organization"), None);
        assert_eq!(sorted_into("team.renamed"), Some(UPDATE));
        assert_eq!(sorted_into("block.updated"), Some(UPDATE));
        assert_eq!(sorted_into("issuing_authorization.created"), Some(CREATE));
        assert_eq!(sorted_into("checkout.session.completed"), Some(LIFECYCLE));
        assert_eq!(sorted_into("member.added"), Some(CREATE));
        assert_eq!(sorted_into("user.session.start"), Some(LIFECYCLE));
    }

    /// Real provider names that sorted as Unsorted before the system list
    /// covered errors, the webhook itself and the integration installing it.
    #[test]
    fn provider_system_names() {
        for event in [
            "meta",                              // GitHub: this hook was deleted
            "installation",                      // GitHub, Sentry
            "installation_repositories",         // GitHub
            "app/uninstalled",                   // Shopify
            "app_uninstalled",                   // Slack
            "endpoint.url_validation",           // Zoom
            "error",                             // Sentry
            "metric_alert",                      // Sentry
        ] {
            assert_eq!(sorted_into(event), Some(SYSTEM), "{event}");
        }
    }

    /// The words left out of the system list on purpose stay where their verb
    /// puts them, and a security word still outranks a system one.
    #[test]
    fn near_misses_stay_out_of_system() {
        assert_eq!(sorted_into("deployment_status"), Some(LIFECYCLE));
        assert_eq!(sorted_into("app_mention"), None);
        assert_eq!(sorted_into("inventory_item.unavailable"), None);
        assert_eq!(sorted_into("spending_limit.updated"), Some(UPDATE));
        assert_eq!(sorted_into("tokens_revoked"), Some(SECURITY));
    }

    /// Real provider names that sorted as Unsorted before the lifecycle list
    /// covered stages in progress, money settling, decisions and the calendar.
    #[test]
    fn provider_lifecycle_names() {
        for event in [
            "payment_intent.processing",              // Stripe
            "payment_intent.requires_action",         // Stripe
            "invoice.payment_action_required",        // Stripe
            "subscription_schedule.released",         // Stripe
            "payment_intent.partially_funded",        // Stripe
            "balance.available",                      // Stripe
            "invoice.finalized",                      // Stripe
            "invoice.voided",                         // Stripe
            "invoice.marked_uncollectible",           // Stripe
            "invoice.sent",                           // Stripe
            "invoice.upcoming",                       // Stripe
            "invoice.overdue",                        // Stripe
            "invoice.will_be_due",                    // Stripe
            "transfer.reversed",                      // Stripe
            "customer.subscription.paused",           // Stripe
            "customer.subscription.resumed",          // Stripe
            "customer.subscription.trial_will_end",   // Stripe
            "customer.source.expiring",               // Stripe
            "identity.verification_session.verified", // Stripe
            "PAYMENT.CAPTURE.PENDING",                // PayPal
            "PAYMENT.CAPTURE.DENIED",                 // PayPal
            "BILLING.SUBSCRIPTION.SUSPENDED",         // PayPal
            "envelope-declined",                      // DocuSign
            "envelope-voided",                        // DocuSign
        ] {
            assert_eq!(sorted_into(event), Some(LIFECYCLE), "{event}");
        }
    }

    /// A stage word loses to a subject word, and to a create, update or delete
    /// verb nearer the end of the name.
    #[test]
    fn near_misses_stay_out_of_lifecycle() {
        assert_eq!(sorted_into("mfa.disabled"), Some(SECURITY));
        assert_eq!(sorted_into("webhook_endpoint.disabled"), Some(SYSTEM));
        assert_eq!(sorted_into("customer.subscription.pending_update_applied"), Some(UPDATE));
        assert_eq!(sorted_into("order.status.updated"), Some(UPDATE));
    }

    /// Real provider names that sorted as Unsorted before the update list
    /// covered attachments, moves and merges, and before the sorter split
    /// camelCase.
    #[test]
    fn provider_update_names() {
        for event in [
            "payment_method.attached",      // Stripe
            "payment_method.detached",      // Stripe
            "inventory_levels/connect",     // Shopify
            "inventory_levels/disconnect",  // Shopify
            "contact.propertyChange",       // HubSpot
            "contact.associationChange",    // HubSpot
            "contact.merge",                // HubSpot
            "updateCard",                   // Trello
            "moveCardToBoard",              // Trello
        ] {
            assert_eq!(sorted_into(event), Some(UPDATE), "{event}");
        }
    }

    /// A later verb from another family still wins, and a pull request that is
    /// merged is a stage rather than an edit.
    #[test]
    fn near_misses_stay_out_of_update() {
        assert_eq!(sorted_into("removeLabelFromCard"), Some(DELETE)); // Trello
        assert_eq!(sorted_into("pull_request.merged"), Some(LIFECYCLE));
        assert_eq!(sorted_into("oauth.token.refreshed"), Some(SECURITY));
        assert_eq!(sorted_into("sync.completed"), Some(LIFECYCLE));
    }

    #[test]
    fn camel_case_splits_at_a_capital_after_lowercase_only() {
        assert_eq!(words("contact.propertyChange"), ["contact", "property", "change"]);
        assert_eq!(words("PAYMENT.CAPTURE.DENIED"), ["payment", "capture", "denied"]);
        assert_eq!(words("oauth2Token"), ["oauth2", "token"]);
        assert_eq!(words("rate_limit.hit"), ["rate", "limit", "hit"]);
    }

    /// Real provider names that sorted as Unsorted before the create list
    /// covered noun forms, arrivals, uploads and copies.
    #[test]
    fn provider_create_names() {
        for event in [
            "contact.creation",      // HubSpot
            "deal.creation",         // HubSpot
            "fork",                  // GitHub
            "form_submission",       // Webflow
            "copyCard",              // Trello
            "team_join",             // Slack
            "member_joined_channel", // Slack
            "subscribe",             // Mailchimp
        ] {
            assert_eq!(sorted_into(event), Some(CREATE), "{event}");
        }
    }

    /// Opening is a stage, not a birth; and a later verb or a subject word
    /// still outranks a create word.
    #[test]
    fn near_misses_stay_out_of_create() {
        assert_eq!(sorted_into("email.opened"), Some(LIFECYCLE)); // Resend
        assert_eq!(sorted_into("review.opened"), Some(LIFECYCLE)); // Stripe
        assert_eq!(sorted_into("form_submission.updated"), Some(UPDATE));
        assert_eq!(sorted_into("api.key.created"), Some(SECURITY));
        assert_eq!(sorted_into("app_mention"), None); // Slack
    }

    #[test]
    fn the_last_verb_decides() {
        assert_eq!(sorted_into("order.status.updated"), Some(UPDATE));
        assert_eq!(sorted_into("deployment_status"), Some(LIFECYCLE));
    }

    #[test]
    fn github_header_names() {
        assert_eq!(sorted_into("create"), Some(CREATE));
        assert_eq!(sorted_into("delete"), Some(DELETE));
        assert_eq!(sorted_into("ping"), Some(SYSTEM));
        assert_eq!(sorted_into("push"), None);
        assert_eq!(sorted_into("pull_request"), None);
    }

    #[test]
    fn case_and_separators_do_not_matter() {
        assert_eq!(sorted_into("Customer.Subscription.DELETED"), Some(DELETE));
        assert_eq!(sorted_into("invoice:payment-failed"), Some(LIFECYCLE));
        assert_eq!(sort("rate_limit.hit").map(|s| s.word), Some("rate".to_string()));
    }
}
