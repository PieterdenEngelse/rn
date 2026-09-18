//! Config → Watching. The pages `watch-pages` fetches, one record each.
//!
//! Its own page rather than rows on Config → Runtime, and the reason is the
//! same one that gave mail rules a page: a watched page is a *record*, and
//! `settings.json` holds scalars. It began as two comma-separated settings,
//! `RN_WATCH_PAGES` and `RN_WATCH_PAGES_IGNORE`, and they could not say the
//! thing people actually want to say — ignore the footer clock on *this* page,
//! check *that* one every fifteen minutes and the terms document weekly.
//!
//! Editing here takes effect on the job's next wake, with no restart. That is
//! the other half of moving off the parameter registry: every runtime parameter
//! is read once at process start, so a watch list living there meant restarting
//! the backend to add a URL.

use crate::api::{delete_page, fetch_jobs, fetch_pages, save_page, JobsResponse, WatchedPage};
use crate::app::Route;
use crate::components::param::{PARAM_INPUT_ROW_CLASS, PARAM_TEXT_INPUT_CLASS};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

/// How often the job itself wakes, read off the catalogue rather than written
/// here — the floor under every per-page interval, and a number this page has
/// no business having its own opinion about.
fn wake_minutes(jobs: &Option<JobsResponse>) -> Option<u32> {
    let s = jobs
        .as_ref()?
        .scheduled
        .iter()
        .find(|s| s.id == "watch-pages")?
        .schedule
        .clone();
    // The schedule arrives as the sentence the page shows elsewhere, so the
    // number is parsed back out of it rather than duplicated. A spelling this
    // does not recognise simply means no floor is quoted, which is better than
    // quoting a wrong one.
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

#[component]
pub fn ConfigWatching() -> Element {
    let mut reload = use_signal(|| 0u32);
    let data = use_resource(move || {
        let _ = reload();
        fetch_pages()
    });
    // Only to say what the job's own cadence is. The page works without it.
    let jobs = use_resource(fetch_jobs);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*data.read_unchecked() {
                Some(Ok(resp)) => {
                    let resp = resp.clone();
                    let wake = wake_minutes(&match &*jobs.read_unchecked() {
                        Some(Ok(j)) => Some(j.clone()),
                        _ => None,
                    });
                    rsx! {
                        Panel {
                            title: "Watched pages".to_string(),
                            subtitle: Some(match resp.pages.len() {
                                0 => "nothing is being watched yet".to_string(),
                                1 => "1 page".to_string(),
                                n => format!("{n} pages"),
                            }),
                            info: Some(rsx! {
                                InfoButton {
                                    title: "Watched pages".to_string(),
                                    what: WATCHING_WHAT.to_string(),
                                    why: WATCHING_WHY.to_string(),
                                    if_wrong: WATCHING_IF_WRONG.to_string(),
                                }
                            }),

                            if resp.pages.is_empty() {
                                p { class: "text-gray-300 max-w-3xl",
                                    "Nothing is watched. Add a page below — the job wakes on its own "
                                    "schedule and fetches whichever pages are past their own interval."
                                }
                            }

                            div { class: "space-y-3",
                                for page in resp.pages.iter() {
                                    PageRow {
                                        key: "{page.id}",
                                        page: page.clone(),
                                        wake,
                                        on_changed: move |_| reload += 1,
                                    }
                                }
                            }
                        }

                        Panel {
                            title: "Add a page".to_string(),
                            subtitle: Some("a URL, and how often to look at it".to_string()),
                            PageForm {
                                page: blank(),
                                wake,
                                adding: true,
                                on_changed: move |_| reload += 1,
                            }
                        }

                        Panel {
                            title: "Where these live".to_string(),
                            p { class: "text-gray-300 max-w-3xl",
                                "Records, not settings: "
                                code { class: "text-gray-200", "{resp.path}" }
                                ". A change here applies on the job's next wake, with no restart — "
                                "unlike anything on "
                                Link {
                                    to: Route::Config {},
                                    class: "text-blue-400 hover:text-blue-300",
                                    "Config → Runtime"
                                }
                                ", which is read once when the process starts."
                            }
                            p { class: "text-gray-400 max-w-3xl mt-2",
                                "The job that reads them is watch-pages, on "
                                Link {
                                    to: Route::MonitorJobs {},
                                    class: "text-blue-400 hover:text-blue-300",
                                    "Monitor → Jobs"
                                }
                                " — its panel has the step-by-step, and its runs are where a change "
                                "is reported."
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    Panel { title: "Watched pages".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Watched pages".to_string(),
                        p { class: "text-gray-400", "Loading…" }
                    }
                },
            }
        }
    }
}

/// A record with the defaults a person would choose, for the add form.
fn blank() -> WatchedPage {
    WatchedPage {
        id: String::new(),
        url: String::new(),
        label: String::new(),
        enabled: true,
        ignore: String::new(),
        only: String::new(),
        // On, because markup comparison reports a change on nearly every
        // request and the first thing a new user would conclude is that the
        // feature is broken.
        text: true,
        every_minutes: 60,
        created_at: 0.0,
    }
}

/// One stored page: what it is, and the two controls that do not need a form.
#[component]
fn PageRow(page: WatchedPage, wake: Option<u32>, on_changed: EventHandler<()>) -> Element {
    let mut editing = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut confirming = use_signal(|| false);

    let toggle = {
        let page = page.clone();
        move |_| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let mut next = page.clone();
            next.enabled = !next.enabled;
            spawn(async move {
                match save_page(&next).await {
                    Ok(r) if r.ok => on_changed.call(()),
                    Ok(r) => error.set(Some(r.errors.join("; "))),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    };

    let remove = {
        let id = page.id.clone();
        move |_| {
            busy.set(true);
            error.set(None);
            let id = id.clone();
            spawn(async move {
                match delete_page(&id).await {
                    Ok(r) if r.ok => on_changed.call(()),
                    Ok(r) => error.set(Some(r.errors.join("; "))),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
                confirming.set(false);
            });
        }
    };

    let shown = if page.label.is_empty() { page.url.clone() } else { page.label.clone() };

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex items-center gap-3 flex-wrap",
                    // No `disabled` attribute anywhere here — see the Form
                    // Control Rules in CLAUDE.md. A paused row says so in
                    // words and colour instead of being greyed into
                    // illegibility.
                    span {
                        class: if page.enabled { "text-gray-200 font-medium" } else { "text-gray-400 font-medium" },
                        "{shown}"
                    }
                    if !page.enabled {
                        span { class: "text-amber-400 text-xs", "paused" }
                    }
                    span { class: "text-gray-400 text-xs", "every {every_phrase(page.every_minutes)}" }
                    if let Some(w) = wake {
                        if page.every_minutes < w {
                            // Said where the number is, not in a panel: a
                            // record asking for more often than the job wakes
                            // is honoured as the job's own cadence, and a row
                            // that quietly rounded up would be lying.
                            span {
                                class: "text-amber-400 text-xs",
                                title: "The job wakes on its own schedule; a page cannot be checked more often than that",
                                "checked every {w}m — the job's own interval"
                            }
                        }
                    }
                    span { class: "text-gray-400 text-xs",
                        if page.text { "comparing text" } else { "comparing markup" }
                    }
                    if !page.only.is_empty() {
                        span { class: "text-gray-300 text-xs", "only lines with “{page.only}”" }
                    }
                    if !page.ignore.is_empty() {
                        span { class: "text-gray-400 text-xs", "ignoring “{page.ignore}”" }
                    }
                }
                div { class: "flex items-center gap-3",
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: toggle,
                        if page.enabled { "Pause" } else { "Resume" }
                    }
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| { let e = editing(); editing.set(!e); },
                        if editing() { "Close" } else { "Edit" }
                    }
                    if confirming() {
                        span { class: "text-amber-400 text-xs", "forget this page?" }
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0 text-red-400",
                            onclick: remove,
                            "Yes, forget it"
                        }
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0 text-gray-300",
                            onclick: move |_| confirming.set(false),
                            "Cancel"
                        }
                    } else {
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0 text-gray-300",
                            onclick: move |_| confirming.set(true),
                            "Remove"
                        }
                    }
                }
            }

            if !page.label.is_empty() {
                p { class: "text-gray-400 text-xs mt-1", "{page.url}" }
            }

            if let Some(e) = error() {
                p { class: "text-amber-400 text-xs mt-2", "{e}" }
            }

            if editing() {
                div { class: "mt-3 pt-3 border-t border-gray-700",
                    PageForm {
                        page: page.clone(),
                        wake,
                        adding: false,
                        on_changed: move |_| { editing.set(false); on_changed.call(()); },
                    }
                }
            }
        }
    }
}

/// The fields of one record, for adding or editing.
#[component]
fn PageForm(
    page: WatchedPage,
    wake: Option<u32>,
    adding: bool,
    on_changed: EventHandler<()>,
) -> Element {
    let mut url = use_signal(|| page.url.clone());
    let mut label = use_signal(|| page.label.clone());
    let mut ignore = use_signal(|| page.ignore.clone());
    let mut only = use_signal(|| page.only.clone());
    let mut text = use_signal(|| page.text);
    let mut every = use_signal(|| page.every_minutes.to_string());
    let mut busy = use_signal(|| false);
    let mut errors = use_signal(Vec::<String>::new);

    let save = {
        let page = page.clone();
        move |_| {
            if busy() {
                return;
            }
            let minutes = every().trim().parse::<u32>().unwrap_or(0);
            if minutes == 0 {
                errors.set(vec!["how often, in minutes — a number of at least 1".to_string()]);
                return;
            }
            busy.set(true);
            errors.set(Vec::new());
            let next = WatchedPage {
                id: page.id.clone(),
                url: url().trim().to_string(),
                label: label().trim().to_string(),
                enabled: page.enabled,
                ignore: ignore().trim().to_string(),
                only: only().trim().to_string(),
                text: text(),
                every_minutes: minutes,
                created_at: page.created_at,
            };
            spawn(async move {
                match save_page(&next).await {
                    Ok(r) if r.ok => {
                        if adding {
                            url.set(String::new());
                            label.set(String::new());
                            ignore.set(String::new());
                            only.set(String::new());
                        }
                        on_changed.call(());
                    }
                    // Named per problem rather than as one "invalid": a record
                    // can be wrong in two ways at once, and fixing them one
                    // refusal at a time is a poor way to spend an afternoon.
                    Ok(r) => errors.set(r.errors),
                    Err(e) => errors.set(vec![e]),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "space-y-2",
            Field {
                label: "URL".to_string(),
                hint: "http or https. The page whose content matters, not a site's front door.".to_string(),
                info: rsx! {
                    InfoButton {
                        title: "URL".to_string(),
                        what: URL_WHAT.to_string(),
                        why: URL_WHY.to_string(),
                        if_wrong: URL_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "text",
                    class: PARAM_TEXT_INPUT_CLASS,
                    placeholder: "https://example.com/status",
                    value: "{url()}",
                    oninput: move |e| url.set(e.value()),
                }
            }
            Field {
                label: "Name".to_string(),
                hint: "optional — what to call it in a report. Empty uses the host and path.".to_string(),
                info: rsx! {
                    InfoButton {
                        title: "Name".to_string(),
                        what: NAME_WHAT.to_string(),
                        why: NAME_WHY.to_string(),
                        if_wrong: NAME_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "text",
                    class: PARAM_TEXT_INPUT_CLASS,
                    placeholder: "Acme status",
                    value: "{label()}",
                    oninput: move |e| label.set(e.value()),
                }
            }
            Field {
                label: "Check every".to_string(),
                hint: match wake {
                    Some(w) => format!("minutes. The job wakes every {w}m, so that is the finest cadence it can honour."),
                    None => "minutes.".to_string(),
                },
                info: rsx! {
                    InfoButton {
                        title: "Check every".to_string(),
                        what: EVERY_WHAT.to_string(),
                        why: EVERY_WHY.to_string(),
                        if_wrong: EVERY_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "number",
                    class: "input input-xs bg-gray-700 text-gray-200",
                    min: "1",
                    value: "{every()}",
                    oninput: move |e| every.set(e.value()),
                }
            }
            Field {
                label: "Watch only lines containing".to_string(),
                hint: "optional — narrows to one part of the page, and lets a report quote it.".to_string(),
                info: rsx! {
                    InfoButton {
                        title: "Watch only lines containing".to_string(),
                        what: ONLY_WHAT.to_string(),
                        why: ONLY_WHY.to_string(),
                        if_wrong: ONLY_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "text",
                    class: PARAM_TEXT_INPUT_CLASS,
                    placeholder: "Status:",
                    value: "{only()}",
                    oninput: move |e| only.set(e.value()),
                }
            }
            Field {
                label: "Ignore lines containing".to_string(),
                hint: "comma-separated, for this page only. The fix for a footer clock.".to_string(),
                info: rsx! {
                    InfoButton {
                        title: "Ignore lines containing".to_string(),
                        what: IGNORE_WHAT.to_string(),
                        why: IGNORE_WHY.to_string(),
                        if_wrong: IGNORE_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "text",
                    class: PARAM_TEXT_INPUT_CLASS,
                    placeholder: "last updated, visitors today",
                    value: "{ignore()}",
                    oninput: move |e| ignore.set(e.value()),
                }
            }
            Field {
                label: "Compare".to_string(),
                hint: "the visible text, rather than the markup.".to_string(),
                info: rsx! {
                    InfoButton {
                        title: "Compare the visible text".to_string(),
                        what: TEXT_WHAT.to_string(),
                        why: TEXT_WHY.to_string(),
                        if_wrong: TEXT_IF_WRONG.to_string(),
                    }
                },
                input {
                    r#type: "checkbox",
                    class: "toggle toggle-sm !border !border-white",
                    style: if text() { "background-color: #0D98BA;" } else { "background-color: #374151;" },
                    checked: text(),
                    onchange: move |e| text.set(e.checked()),
                }
            }

            if !errors().is_empty() {
                div { class: "rounded border border-amber-500 bg-gray-900 p-3 space-y-1",
                    for e in errors().iter() {
                        p { class: "text-gray-200 text-xs", "• {e}" }
                    }
                }
            }

            div {
                button {
                    class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                    style: "background-color: #026B7C;",
                    onclick: save,
                    if busy() {
                        "Saving…"
                    } else if adding {
                        "Add this page"
                    } else {
                        "Save"
                    }
                }
            }
        }
    }
}

/// One labelled control, with its hint and its panel in the info column.
#[component]
fn Field(label: String, hint: String, info: Element, children: Element) -> Element {
    rsx! {
        div { class: PARAM_INPUT_ROW_CLASS,
            div { class: "flex items-center gap-3 flex-wrap",
                span { class: "text-gray-300 text-xs w-32 shrink-0", "{label}" }
                {children}
                span { class: "text-gray-400 text-xs", "{hint}" }
            }
            {info}
        }
    }
}

/// "45 minutes", "2 hours", "3 days" — the interval as somebody would say it.
fn every_phrase(minutes: u32) -> String {
    if minutes < 60 {
        return format!("{minutes}m");
    }
    if minutes % 1440 == 0 {
        let days = minutes / 1440;
        return if days == 1 { "day".to_string() } else { format!("{days} days") };
    }
    if minutes % 60 == 0 {
        let hours = minutes / 60;
        return if hours == 1 { "hour".to_string() } else { format!("{hours} hours") };
    }
    format!("{}h{}m", minutes / 60, minutes % 60)
}

const WATCHING_WHAT: &str =
    "The pages watch-pages fetches, one record each: the URL, whether it is on, what noise to \
     ignore on that page, and how often to check it. The job holds no list of its own — it wakes \
     on its schedule and fetches whichever records are past their own interval, which is usually \
     none of them.\n\nEach page is compared against what it looked like last time. What is \
     remembered is a hash and a couple of numbers, not the page, which is why a report says that \
     a page changed and by roughly how much rather than what changed.";

const WATCHING_WHY: &str =
    "Most of what is worth watching does not publish a feed and has no version number. A status \
     page, a pricing table, the terms you agreed to, a vacancy list: they change by somebody \
     editing a page and telling nobody.\n\nThese are records rather than settings because the \
     interesting parts belong to one page rather than to all of them. \"Last updated\" is noise \
     in one site's footer and the entire point of a changelog; a status page is worth fifteen \
     minutes and a terms document is worth a week. Two comma-separated settings — which is what \
     this was — could say neither.\n\nA change here applies on the job's next wake. Nothing needs \
     restarting, which is the other half of not being a runtime parameter.";

const WATCHING_IF_WRONG: &str =
    "The failure that matters is a page that renders with JavaScript. The job fetches HTML and \
     runs none of it, so such a page reads as a small document that never changes — forever, and \
     confidently. Its first run on a page reports the character count, and a number like 300 on a \
     page you know is full of text is the tell. There is no fix here: watch an underlying API or \
     feed instead, if the site has one.\n\nA page switched off is paused rather than forgotten — \
     what the job remembers survives, so resuming it reports everything that changed meanwhile as \
     one change. Removing the record is how you say \"stop, and forget where this stood\".\n\nA \
     page asking to be checked more often than the job wakes gets the job's cadence instead, and \
     the row says so rather than pretending.";

const URL_WHAT: &str =
    "The page to fetch, as an http or https URL. It is fetched exactly as written — no link is \
     followed, no form is submitted, and nothing else on the site is read.\n\nA URL that is not \
     http or https is refused when you save it rather than at four in the morning. That is the \
     point of validating here: a mistake is answered while the person who made it is looking at \
     it.";

const URL_WHY: &str =
    "Point it at the page whose content you care about rather than at a site's front door. A home \
     page changes when anything anywhere on the site changes, which is a notification that means \
     nothing and trains you to ignore the next one.\n\nThe query string is part of the identity, \
     so a URL with a session token or a tracking parameter in it is a different page from the \
     same one without — and a token that expires turns into a page that starts failing.";

const URL_IF_WRONG: &str =
    "Editing the URL keeps the record's history: what the job remembers is keyed to the record, \
     not to the address, so fixing a typo does not restart the comparison from nothing.\n\nA page \
     that answers 404 or 500 is reported as a failed fetch and keeps its old mark, so a site \
     having a bad afternoon does not erase what it looked like before.";

const NAME_WHAT: &str =
    "What this page is called in a report and on this list. Empty falls back to the host and \
     path, which is what the job's own steps use.";

const NAME_WHY: &str =
    "A URL is a poor thing to read in a notification at seven in the morning. \"Acme status\" \
     says what moved; \"status.acme-corp.io/incidents/current\" makes you parse it.";

const NAME_IF_WRONG: &str =
    "It is decoration and nothing depends on it. Changing it does not affect what is watched or \
     what is remembered — the record's identity is its id, which nothing here shows because \
     nothing here needs it.";

const EVERY_WHAT: &str =
    "How often this page should be fetched, in minutes. A page is fetched when that long has \
     passed since it was last read — which is a different question from when it last changed, and \
     both are remembered.\n\nIt is a floor rather than a promise. The job only looks when it \
     wakes, so a page asking for less than the job's own interval is fetched on that interval \
     instead.";

const EVERY_WHY: &str =
    "Pages differ, and this is the whole reason they are records. A status page during an \
     incident is worth every fifteen minutes; a terms document is worth a week. One interval for \
     both means either hammering somebody's server for a document that changes twice a year, or \
     hearing about the outage tomorrow.\n\nIt is also basic manners. Fetching a stranger's page \
     every minute for something that changes monthly is the kind of thing that gets a user agent \
     blocked, and rn's user agent names this project.";

const EVERY_IF_WRONG: &str =
    "Too long and you hear about a change late, which for a status page is the same as not \
     hearing. Too short and you are making requests nobody needed.\n\nThe interval is measured \
     from the last successful fetch, so a page that has been failing is due again on the next \
     wake rather than waiting out another interval on the strength of a fetch that did not \
     happen.";

const ONLY_WHAT: &str =
    "Comma-separated substrings. When this is set, only the lines containing one of them are \
     compared — the rest of the page is not watched at all. Empty watches the whole page, which \
     is the right default for \"tell me if anything here moves\".\n\nIt is applied after the \
     ignore list, so a record can use both: drop the line reading \"price updated at 14:05\", \
     keep the one reading \"price\".";

const ONLY_WHY: &str =
    "Two reasons, and the second is the one people do not expect.\n\nThe first is precision. A \
     page you care about one fact on — a status, a version, a price — reports a change whenever \
     anything else on it moves, and a report you have learned to distrust is one you stop \
     opening. Naming the line makes the answer mean something.\n\nThe second is that a \
     selection is small enough to keep. The job stores a hash of what it compared, not the page, \
     because a page per URL is exactly the growth the store refuses — so a whole-page watch can \
     only ever tell you the size moved. A line or two can be stored, and then the report quotes \
     it: \"Status: operational\" became \"Status: degraded\", which is the sentence you actually \
     wanted. The quote is capped at 400 characters; past that it compares as usual and does not \
     quote.";

const ONLY_IF_WRONG: &str =
    "A phrase that matches nothing means nothing is being compared. That is reported as an \
     empty selection rather than passed over, because otherwise the comparison quietly becomes \
     empty against empty and never reports again — the page would look permanently unchanged.\n\n\
     It matches the reduced text, so with the text comparison on you are matching what a reader \
     sees, not the markup: a word that only appears in a class name or an attribute will not \
     match. Match on the visible words instead, and prefer a phrase the page shows anyway — the \
     label beside the value, rather than the value, which is the thing that is about to change.";

const IGNORE_WHAT: &str =
    "Comma-separated substrings, for this page only. Any line containing one of them is dropped \
     before the page is compared, matched without regard to case.\n\nPlain substrings rather than \
     patterns: a regular expression typed into a field is a way to hang the job on the page it \
     was pointed at, and what people mean is nearly always \"the line with the word Updated in \
     it\".";

const IGNORE_WHY: &str =
    "It is the fix for the one page that keeps reporting when nothing happened. A footer reading \
     \"Last updated 14:05\", a visitor counter, a copyright year — one entry here turns an hourly \
     false alarm into silence without giving up the rest of the page.\n\nStart empty, wait for a \
     false report, then ignore the line it was about. Guessing in advance mostly removes lines \
     that were never going to move.";

const IGNORE_IF_WRONG: &str =
    "Too broad and you lose the change you were watching for: ignoring \"price\" on a pricing \
     page drops the row that matters along with the noise, and the run then reports nothing \
     rather than reporting less.\n\nBecause it belongs to this page alone, a word that is noise \
     here and content elsewhere costs nothing — which is exactly what the install-wide setting \
     this replaced could not do.";

const TEXT_WHAT: &str =
    "On, the page is reduced to roughly what a reader sees before it is compared: script and \
     style blocks removed, comments removed, tags removed, entities decoded, whitespace \
     collapsed. Off, the raw markup is compared exactly as it arrived.";

const TEXT_WHY: &str =
    "On is what you want almost always, because almost every page differs on every request in \
     ways nobody means: a CSRF token, a build id in an asset URL, an ad slot, a rendered \
     timestamp. Comparing markup reports all of it, every time, and a watcher that reports a \
     change on every run is one you stop reading.\n\nOff is for when the markup is the point — a \
     canonical link moving, a script source changing, a meta tag appearing. Those are invisible \
     in the text and are sometimes exactly what is being watched for.";

const TEXT_IF_WRONG: &str =
    "Left on where you needed markup, a change you cared about never reports and nothing says so \
     — the run looks like a page that did not move.\n\nTurned off on an ordinary page, expect a \
     change reported nearly every run, and the ignore list is then the only thing between you and \
     constant noise.";
