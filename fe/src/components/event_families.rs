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
        words: &["created", "create", "registered", "added", "add", "new", "opened", "inserted"],
        by_subject: false,
        asks: "safe to run twice — a retried delivery is the same order again",
        kind: WebhookKind::DataPayload,
        kind_why: "the new record is in the body — there is nothing to go back for",
        what: "A provider telling you a record now exists that did not before — a payment, an \
               order, an account. The verb is usually the last word of the name: \
               `payment.created`, `user.registered`. GitHub is the exception worth knowing: its \
               event header says just `create`, with no noun, and means a branch or tag.",
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
            "updated", "update", "changed", "change", "edited", "modified", "renamed", "moved",
            "replaced", "synchronize",
        ],
        by_subject: false,
        asks: "act on the current state — two edits can arrive in either order",
        kind: WebhookKind::Notification,
        kind_why: "rn fetches the record when the delivery lands, so an older edit arriving late cannot win",
        what: "Something that already existed changed: an invoice's amount, a subscription's \
               plan, an order's status. Usually `<noun>.updated` or `<noun>.changed`; some \
               providers name the field as well, as in `order.status.updated`, which is why the \
               last verb in the name decides rather than the first word.",
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
            "succeeded", "success", "failed", "failure", "completed", "complete", "started",
            "finished", "delivered", "shipped", "paid", "cancelled", "canceled", "expired",
            "refunded", "approved", "rejected", "activated", "deactivated", "closed", "reopened",
            "resolved", "published", "fulfilled", "captured", "confirmed", "status",
        ],
        by_subject: false,
        asks: "one transition can arrive as two events — pick one to act on",
        kind: WebhookKind::DataPayload,
        kind_why: "the stage is in the name and the resource in the body",
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
        words: &[
            "login", "logout", "signin", "password", "passwd", "mfa", "2fa", "otp", "token",
            "key", "secret", "credential", "credentials", "auth", "oauth", "permission",
            "permissions", "role", "sso", "security", "suspicious", "fraud", "lockout", "locked",
        ],
        by_subject: true,
        asks: "worth a person seeing now — route it to a notifier",
        kind: WebhookKind::DataPayload,
        kind_why: "act on what arrived at once — a lookup is a second call that can fail at the worst moment",
        what: "Something happened to access itself: a login attempt, a password change, an API \
               key revoked. Recognised by the subject rather than the verb, and checked before \
               any verb, so `password.changed` is here rather than under Update and \
               `api.key.created` rather than under Create.",
        why: "The events worth acting on quickly and worth a person seeing — a notifier job \
              (desktop-notify, notify-mail) rather than a report nobody opens. They are also \
              where a forged delivery would do the most harm, which is why every hook on the \
              listener checks a signature or token and none accepts an unsigned delivery.",
        if_wrong: "A revoked key a job goes on using until it fails, or a login from somewhere \
                   unexpected that nobody hears about. A security event under Unsorted on \
                   Monitor → Webhooks means its name uses a word this list does not have.",
    },
    EventFamily {
        id: WebhookFamily::System,
        name: "System events",
        examples: "server.alert; quota.exceeded; rate_limit.hit",
        meaning: "System behaviour or internal alert",
        words: &[
            "server", "system", "quota", "rate", "ratelimit", "throttled", "exceeded", "incident",
            "outage", "maintenance", "degraded", "health", "ping", "alert",
        ],
        by_subject: true,
        asks: "high volume — filter at the provider, not inside the job",
        kind: WebhookKind::DataPayload,
        kind_why: "high volume — a lookup per delivery doubles the cost of every burst",
        what: "The provider talking about itself or about your account's limits: a server \
               raising an alarm, a quota exceeded, a rate limit hit. Recognised by words like \
               server, quota and rate anywhere in the name. GitHub's `ping`, sent once when a \
               hook is made, lands here too.",
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
/// `["rate", "limit", "hit"]`.
fn words(event: &str) -> Vec<String> {
    event
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect()
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
