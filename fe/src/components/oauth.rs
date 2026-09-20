//! Config → Connection's OAuth panel: sign in to a provider once, and a job
//! gets the token.
//!
//! The mechanism lives in `be/src/oauth.ts`; this is the part a person sees,
//! and its job is to make five hops across two hosts and a browser legible —
//! what to register, what this backend will send, where the token lands, which
//! jobs read it, and, when a sign-in fails, which hop it stopped at.
//!
//! Nothing here can show a token. The backend sends who it signed in as, the
//! scopes, and when it dies; the token went into an ordinary credential and
//! the only shape that could carry it back does not exist.

use crate::api::{
    disconnect_oauth, fetch_oauth, save_credential, start_oauth, JobStage, OAuthAttempt,
    OAuthConnection, OAuthProvider,
};
use crate::clipboard::copy_to_clipboard;
use crate::components::param::{PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS, PARAM_INPUT_ROW_CLASS, PARAM_TEXT_INPUT_CLASS};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

const HINT: &str = "text-gray-400 text-xs";
const CYAN: &str = "color: #22d3ee;";
const TEXT_ACTION: &str = "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0";

/// The panel: one board per provider.
#[component]
pub fn OAuthPanel() -> Element {
    let mut reload = use_signal(|| 0u32);
    let providers = use_resource(move || {
        reload();
        fetch_oauth()
    });

    rsx! {
        Panel {
            title: "OAuth".to_string(),
            subtitle: Some("sign in once, and a job gets the token".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "OAuth sign-in".to_string(),
                    what: PANEL_WHAT.to_string(),
                    why: PANEL_WHY.to_string(),
                    if_wrong: PANEL_IF_WRONG.to_string(),
                    // The whole flow, a tab per hop. The Overview's five-line
                    // version is the shape of it; these are the parameters,
                    // the routes and the refusals, which is what anyone
                    // debugging a sign-in actually needs.
                    stages: flow_stages(),
                }
            }),
            match &*providers.read_unchecked() {
                Some(Ok(r)) => rsx! {
                    div { class: "flex flex-wrap gap-4",
                        for p in r.providers.iter() {
                            ProviderBoard {
                                key: "{p.id}",
                                provider: p.clone(),
                                on_change: move |_| reload += 1,
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400", "Could not read /api/oauth: {e}" }
                    p { class: "max-w-3xl text-gray-400 mt-1",
                        "A backend started before OAuth existed has no such route — the launcher \
                         serves whatever it was started with. Restart it and this fills in."
                    }
                },
                None => rsx! { p { class: "text-gray-400", "Loading…" } },
            }
        }
    }
}

/// "4m ago", from an epoch-millisecond instant in the past.
fn ago(ms: f64) -> String {
    let secs = ((js_sys::Date::now() - ms) / 1000.0).max(0.0) as i64;
    match secs {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

/// "in 7h 40m", or "3m ago" once past.
fn until(ms: f64) -> String {
    let secs = ((ms - js_sys::Date::now()) / 1000.0) as i64;
    if secs < 0 {
        return format!("expired {}", ago(ms));
    }
    let mins = secs / 60;
    match mins {
        m if m < 60 => format!("in {m}m"),
        m if m < 48 * 60 => format!("in {}h {}m", m / 60, m % 60),
        m => format!("in {}d", m / (24 * 60)),
    }
}

fn connection_line(c: &OAuthConnection) -> String {
    let who = c
        .login
        .as_ref()
        .map(|l| format!("as {l}"))
        .unwrap_or_else(|| "account unknown".to_string());
    let expiry = match c.expires_at_ms {
        Some(at) => format!("token expires {}", until(at)),
        None => "no expiry given".to_string(),
    };
    format!("{who} · signed in {} · {expiry}", ago(c.connected_at_ms))
}

fn granted_line(c: &OAuthConnection) -> String {
    if c.scopes.is_empty() {
        "no scopes — public data only".to_string()
    } else {
        c.scopes.join(", ")
    }
}

fn refresh_line(c: &OAuthConnection) -> String {
    match (c.refreshable, c.refresh_expires_at_ms) {
        (true, Some(at)) => format!(
            "refresh token held, renewed before a run that needs it · it expires {}",
            until(at)
        ),
        (true, None) => "refresh token held, renewed before a run that needs it".to_string(),
        (false, _) => {
            "no refresh token — this grant does not need one, or signs in again when it dies".to_string()
        }
    }
}

/// One provider: what it needs, what it has, and the button.
#[component]
fn ProviderBoard(provider: OAuthProvider, on_change: EventHandler<()>) -> Element {
    let p = provider;
    let mut scopes = use_signal({
        let d = p.default_scopes.clone();
        move || d
    });
    let mut errors = use_signal(Vec::<String>::new);
    let mut notice = use_signal(|| Option::<String>::None);
    let mut busy = use_signal(|| false);

    let ready = p.client_id_set && p.client_secret_set && p.redirect_uri.is_some();
    let id = p.id.clone();
    let mut connect = move |_| {
        let id = id.clone();
        let wanted = scopes();
        busy.set(true);
        errors.write().clear();
        spawn(async move {
            match start_oauth(&id, &wanted).await {
                Ok(r) if r.ok => {
                    if let (Some(url), Some(win)) = (r.authorize_url, web_sys::window()) {
                        // The same tab, not a new one: the provider sends this
                        // tab back to the page, and a second tab would leave
                        // the first showing a board from before the sign-in.
                        let _ = win.location().set_href(&url);
                        return;
                    }
                    errors.set(vec!["the backend answered without a URL".to_string()]);
                }
                Ok(r) => errors.set(r.errors),
                Err(e) => errors.set(vec![e]),
            }
            busy.set(false);
        });
    };
    let id = p.id.clone();
    let mut disconnect = move |_| {
        let id = id.clone();
        busy.set(true);
        spawn(async move {
            match disconnect_oauth(&id).await {
                Ok(r) => notice.set(Some(r.detail)),
                Err(e) => errors.set(vec![e]),
            }
            busy.set(false);
            on_change.call(());
        });
    };

    let token_state = match (&p.connection, p.token_set) {
        (Some(c), _) => ("text-gray-200", format!("connected {}", connection_line(c))),
        (None, true) => (
            "text-gray-300",
            format!("{} is set, but not by a sign-in here", p.token_credential),
        ),
        (None, false) => ("text-gray-400", "not connected".to_string()),
    };

    rsx! {
        div { class: "{PARAM_BOARD_BASE_CLASS} flex-1 min-w-96 space-y-2",
            div { class: "flex items-center gap-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{p.label}" }
                span { class: "text-xs {token_state.0}", "{token_state.1}" }
            }

            ClientCredentialRow {
                label: "Client ID".to_string(),
                name: p.client_id_credential.clone(),
                set: p.client_id_set,
                secret: false,
                on_saved: move |_| on_change.call(()),
                info: rsx! {
                    InfoButton {
                        title: "Client ID".to_string(),
                        what: CLIENT_ID_WHAT.to_string(),
                        why: CLIENT_ID_WHY.to_string(),
                        if_wrong: CLIENT_ID_IF_WRONG.to_string(),
                    }
                },
            }
            ClientCredentialRow {
                label: "Client secret".to_string(),
                name: p.client_secret_credential.clone(),
                set: p.client_secret_set,
                secret: true,
                on_saved: move |_| on_change.call(()),
                info: rsx! {
                    InfoButton {
                        title: "Client secret".to_string(),
                        what: CLIENT_SECRET_WHAT.to_string(),
                        why: CLIENT_SECRET_WHY.to_string(),
                        if_wrong: CLIENT_SECRET_IF_WRONG.to_string(),
                    }
                },
            }

            div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
                div { class: "flex flex-col gap-0.5",
                    div { class: "flex items-baseline gap-3 flex-wrap",
                        span { class: "text-gray-200 text-sm", "Callback URL to register" }
                        code { class: "text-gray-300 text-xs", "{p.register_callback}" }
                        button {
                            class: TEXT_ACTION,
                            style: CYAN,
                            onclick: {
                                let url = p.register_callback.clone();
                                move |_| copy_to_clipboard(&url)
                            },
                            "Copy"
                        }
                    }
                    if let Some(note) = p.register_note.as_ref() {
                        div { class: HINT, "{note}" }
                    }
                    div { class: HINT,
                        "no port: {p.label} matches a loopback callback on any port · register the app at "
                        a {
                            class: "text-blue-400 hover:text-blue-300",
                            href: "{p.register_at}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            "{p.register_at}"
                        }
                    }
                }
                InfoButton {
                    title: "Callback URL".to_string(),
                    what: CALLBACK_WHAT.to_string(),
                    why: CALLBACK_WHY.to_string(),
                    if_wrong: CALLBACK_IF_WRONG.to_string(),
                }
            }

            div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 text-sm", "This backend sends" }
                    match (&p.redirect_uri, &p.redirect_problem) {
                        (Some(uri), _) => rsx! { code { class: "text-gray-300 text-xs", "{uri}" } },
                        (None, Some(problem)) => rsx! { span { class: "text-amber-400 text-xs", "{problem}" } },
                        (None, None) => rsx! { span { class: HINT, "no redirect URI" } },
                    }
                }
                InfoButton {
                    title: "Redirect URI".to_string(),
                    what: REDIRECT_WHAT.to_string(),
                    why: REDIRECT_WHY.to_string(),
                    if_wrong: REDIRECT_IF_WRONG.to_string(),
                }
            }

            div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
                div { class: "flex items-center gap-3 flex-wrap",
                    span { class: "text-gray-200 text-sm", "Scopes" }
                    input {
                        r#type: "text",
                        class: PARAM_TEXT_INPUT_CLASS,
                        spellcheck: "false",
                        value: "{scopes}",
                        oninput: move |e| scopes.set(e.value()),
                    }
                    span { class: HINT, "space-separated; empty asks for public data only" }
                }
                InfoButton {
                    title: "Scopes".to_string(),
                    what: SCOPES_WHAT.to_string(),
                    why: SCOPES_WHY.to_string(),
                    if_wrong: SCOPES_IF_WRONG.to_string(),
                }
            }

            div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 text-sm", "Token goes into" }
                    code { class: "text-gray-300 text-xs", "{p.token_credential}" }
                    if p.used_by.is_empty() {
                        span { class: HINT, "— nothing reads it yet" }
                    } else {
                        span { class: HINT, "— read by {p.used_by.join(\", \")}" }
                    }
                }
                InfoButton {
                    title: "Where the token goes".to_string(),
                    what: TOKEN_WHAT.to_string(),
                    why: TOKEN_WHY.to_string(),
                    if_wrong: TOKEN_IF_WRONG.to_string(),
                }
            }

            if let Some(c) = &p.connection {
                div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
                    div { class: "flex flex-col gap-0.5",
                        div { class: "flex items-baseline gap-3 flex-wrap",
                            span { class: "text-gray-200 text-sm", "Granted" }
                            span { class: "text-gray-300 text-xs", "{granted_line(c)}" }
                        }
                        div { class: HINT, "{refresh_line(c)}" }
                        if let Some(r) = &c.last_refresh {
                            AttemptLines { attempt: r.clone(), heading: "Last renewal".to_string() }
                        }
                    }
                    InfoButton {
                        title: "Granted, and renewal".to_string(),
                        what: GRANTED_WHAT.to_string(),
                        why: GRANTED_WHY.to_string(),
                        if_wrong: GRANTED_IF_WRONG.to_string(),
                    }
                }
            }

            div { class: "{PARAM_INPUT_ROW_CLASS} pt-1",
                div { class: "flex items-center gap-4 flex-wrap",
                    button {
                        class: if ready && !busy() { "btn btn-primary btn-sm" } else { "btn btn-primary btn-sm btn-disabled" },
                        // Not the HTML `disabled` attribute — see Form Control
                        // Rules; the handler checks instead.
                        onclick: move |e| if ready && !busy() { connect(e) },
                        if busy() { "Working…" } else if p.connection.is_some() { "Sign in again" } else { "Sign in with {p.label}" }
                    }
                    if p.connection.is_some() || p.token_set {
                        button {
                            class: TEXT_ACTION,
                            style: CYAN,
                            onclick: move |e| if !busy() { disconnect(e) },
                            "Disconnect"
                        }
                    }
                    if !ready {
                        span { class: HINT,
                            if p.redirect_uri.is_none() {
                                "no loopback redirect is possible with this bind address"
                            } else {
                                "set the client ID and secret first"
                            }
                        }
                    }
                    if p.pending > 0 {
                        span { class: HINT,
                            "{p.pending} sign-in open, valid {p.pending_ttl_seconds / 60} minutes from when it started"
                        }
                    }
                }
                InfoButton {
                    title: "Signing in".to_string(),
                    what: SIGNIN_WHAT.to_string(),
                    why: SIGNIN_WHY.to_string(),
                    if_wrong: SIGNIN_IF_WRONG.to_string(),
                }
            }

            if !errors().is_empty() {
                div { class: "rounded border border-red-500 bg-gray-900 p-3 space-y-1",
                    for e in errors().iter() {
                        p { class: "text-gray-200 text-sm", "• {e}" }
                    }
                }
            }
            if let Some(n) = notice() {
                p { class: "text-gray-300 text-sm", "{n}" }
            }
            if let Some(a) = &p.last_attempt {
                AttemptLines { attempt: a.clone(), heading: "Last sign-in".to_string() }
            }
        }
    }
}

/// What an attempt did, hop by hop, with the last line being where it stopped.
#[component]
fn AttemptLines(attempt: OAuthAttempt, heading: String) -> Element {
    let a = attempt;
    rsx! {
        div { class: "mt-1",
            div { class: "flex items-baseline gap-3 flex-wrap",
                span { class: "text-gray-300 text-xs font-semibold", "{heading}" }
                span {
                    class: if a.ok { "text-gray-200 text-xs" } else { "text-red-400 text-xs" },
                    "{a.detail}"
                }
                span { class: HINT, "{ago(a.at_ms)}" }
            }
            if !a.steps.is_empty() {
                ol { class: "list-decimal ml-5 text-gray-400 text-xs space-y-0.5 mt-1",
                    for (i, s) in a.steps.iter().enumerate() {
                        li { key: "{i}", "{s}" }
                    }
                }
            }
        }
    }
}

/// The app's client ID or secret: set or not, and a write-only way to set it.
///
/// The same endpoint as the credentials board, so the value
/// takes the same path — into the process first, arming redaction, then into
/// the file. A client ID is not secret, and is stored as a credential anyway
/// so both halves of the app live in one place and a person sets them the same
/// way.
#[component]
fn ClientCredentialRow(
    label: String,
    name: String,
    set: bool,
    secret: bool,
    on_saved: EventHandler<()>,
    info: Element,
) -> Element {
    let mut open = use_signal(|| false);
    let mut value = use_signal(String::new);
    let mut error = use_signal(|| Option::<String>::None);
    let mut busy = use_signal(|| false);

    let save = {
        let name = name.clone();
        move |_| {
            let v = value();
            if v.trim().is_empty() {
                error.set(Some("nothing was typed".to_string()));
                return;
            }
            let name = name.clone();
            busy.set(true);
            spawn(async move {
                match save_credential(&name, v.trim()).await {
                    Ok(r) if r.ok => {
                        error.set(None);
                        open.set(false);
                        on_saved.call(());
                    }
                    Ok(r) => error.set(Some(r.errors.join("; "))),
                    Err(e) => error.set(Some(e)),
                }
                // Cleared whatever happened: a value left in a signal is a
                // value still in the page's memory.
                value.set(String::new());
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
            div { class: "flex flex-col gap-1",
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 text-sm", "{label}" }
                    span {
                        class: if set { "text-gray-300 text-xs" } else { "text-red-400 text-xs" },
                        if set { "set" } else { "not set" }
                    }
                    code { class: "text-gray-400 text-xs", "{name}" }
                    button {
                        class: TEXT_ACTION,
                        style: CYAN,
                        onclick: move |_| {
                            value.set(String::new());
                            error.set(None);
                            open.set(!open());
                        },
                        if open() { "Cancel" } else if set { "Replace" } else { "Set" }
                    }
                }
                if open() {
                    div { class: "flex items-center gap-3 flex-wrap",
                        input {
                            // Password-typed for the secret, so a screen-share
                            // sees dots; plain for the ID, which is public and
                            // easier to check by eye.
                            r#type: if secret { "password" } else { "text" },
                            class: PARAM_TEXT_INPUT_CLASS,
                            autocomplete: "off",
                            spellcheck: "false",
                            placeholder: "paste it from the app's settings",
                            oninput: move |e| value.set(e.value()),
                        }
                        button {
                            class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                            style: "background-color: #026B7C;",
                            onclick: save,
                            if busy() { "Saving…" } else { "Save" }
                        }
                    }
                }
                if let Some(e) = error() {
                    span { class: "text-red-400 text-xs", "{e}" }
                }
            }
            {info}
        }
    }
}

/// The authorization-code flow as rn runs it, one tab per hop.
///
/// Written from `be/src/oauth.ts` rather than from the RFC: the parameters
/// named here are the ones this backend sends, the routes are its routes, and
/// the refusals are the ones it makes. A generic OAuth explainer is a search
/// away and would not say which of five hops a sign-in stopped at.
fn stage(name: &str, lead: &str, body: &str) -> JobStage {
    JobStage { name: name.to_string(), lead: lead.to_string(), body: body.to_string(), reports: None }
}

fn flow_stages() -> Vec<JobStage> {
    vec![
        stage(
            "Register the app",
            "Once per provider, on the provider's own site.",
            "A sign-in needs an app to sign in to. The provider gives back a client ID, which is \
             public, and a client secret, which is not; both go on the board below, where they \
             are stored like any other credential and neither is ever rendered back.\n\nThe \
             callback URL is what the provider will send the browser to. It is shown on each \
             board with a Copy button, without a port: GitHub matches a loopback callback on \
             host and path and lets the port vary, which is what lets four worktrees on four API \
             ports share one app. Google only does that for a client registered as a Desktop \
             app — a Web application client matches the redirect URI exactly, port and all — and \
             its board says so.\n\nNothing is registered from here. rn cannot create an app on \
             somebody else's site, and a page that pretended to would be a form that silently \
             did nothing.",
        ),
        stage(
            "Start",
            "POST /api/oauth/:id/start — the only request that mints anything.",
            "The button sends the scopes from the box beside it. A POST rather than a GET on \
             purpose: it creates state, and a link, a prefetch or a preview should not be able \
             to.\n\nThe backend refuses here, before any browser leaves the page, if the client \
             ID or secret is unset, if the scopes hold anything outside letters, digits and \
             : _ . / -, or if the API is bound to a non-loopback address — there is no \
             redirect to offer in that last case, and the board says which bind address caused \
             it.\n\nOtherwise it makes two random 32-byte values, base64url: state, and a \
             PKCE code_verifier. It remembers them in memory against the provider, the exact \
             redirect URI, the scopes and the page to return to, and answers with a URL.",
        ),
        stage(
            "The authorize URL",
            "What the browser carries to the provider, parameter by parameter.",
            "response_type=code, always — Google refuses an authorize request without it and \
             GitHub assumes it. client_id. redirect_uri, the exact loopback URL the callback \
             will arrive on, because the provider compares it again at the exchange. scope, \
             normalised to space-separated. state. code_challenge, the SHA-256 of the \
             verifier, base64url, with code_challenge_method=S256.\n\nThen whatever that \
             provider wants of its own: Google gets access_type=offline and prompt=consent, \
             without which it returns an access token that dies in an hour and no refresh token \
             — a sign-in that looks fine and is dead by lunchtime.\n\nThe client secret is not \
             in this URL and never is. It goes to the token endpoint, from the backend, over \
             TLS.\n\nThe page then navigates this tab to it. The same tab rather than a new \
             one, so the board you started from is the board that shows the result.",
        ),
        stage(
            "Consent",
            "At the provider. rn is not in this hop at all.",
            "You authenticate to the provider — password, passkey, whatever they use, plus \
             whatever second factor — and approve the scopes. rn never sees any of it, which is \
             the property that makes a sign-in better than a password in a box.\n\nYou can \
             narrow the scopes here, and providers let you: what is granted is what the consent \
             screen agreed to, not what was asked for. That is why the board reports granted \
             scopes separately, and why mail checks for https://mail.google.com/ among them \
             rather than assuming the sign-in asked for it.\n\nDeclining is an ordinary answer. \
             The provider sends the browser back with error=access_denied instead of a code, \
             and the panel records it in words rather than as a failure to debug.",
        ),
        stage(
            "The redirect back",
            "GET http://127.0.0.1:<API port>/api/oauth/:id/callback?code=…&state=…",
            "This is the hop the flow was once ruled out for. It is an inbound unsigned GET — \
             but not from the internet. The provider redirects the browser itself, and the browser is \
             on this machine, so the request comes from loopback to the API listener the page \
             already talks to. RFC 8252 is the standard for exactly this shape. The hooks \
             listener, whose safety is that it verifies a signature on every call, is \
             untouched.\n\nWhat a loopback callback does add is one caller the API did not have: \
             a web page in this browser can navigate to it. It could not read the answer, but it \
             could hand rn a code for its own account, and every job would then run as \
             someone else — login CSRF. state is the answer: the value has to be one this \
             process issued, it is spent the moment it is looked up, whatever happens next, and \
             it expires after ten minutes. A callback rn did not start is refused with a short \
             plain-text page, and is deliberately not recorded as an attempt — a page that could \
             write failures into this panel by opening a URL would be a way to mislead \
             you.\n\nAt most eight sign-ins may be in flight at once, oldest dropped past that, \
             so a loop calling start cannot grow the map without bound.",
        ),
        stage(
            "Exchange the code",
            "An outbound POST to the provider's token endpoint — the direction that always worked.",
            "Form-encoded: grant_type=authorization_code, the code, the client_id and \
             client_secret, the same redirect_uri the authorize URL carried, and the \
             code_verifier — the value whose hash went out at the start and which has not left \
             this process. A code intercepted on the way back is useless without \
             it.\n\naccept: application/json is not optional: without it GitHub answers \
             form-encoded and every field reads as absent. GitHub also answers a refused \
             exchange with 200 and an error field, so the body is read either way rather \
             than trusting the status. Fifteen seconds, then the attempt is abandoned; a \
             runtime that refuses the outbound call is named with the host to allow.",
        ),
        stage(
            "Store the token",
            "Into an ordinary credential — not a second store.",
            "The access token is written with the same writer the credentials board uses: into \
             this process's environment first, which is what arms redaction before the value \
             touches disk, then into ~/.config/rn/credentials. So ctx.secret(\"githubToken\") \
             does not know or care that a sign-in filled it, and no job changed to gain \
             one.\n\nA refresh token, where the provider sends one, goes to its own credential \
             beside it. A sign-in that returns none clears any old one, because it belonged to \
             a grant this one replaced; a refresh that returns none keeps the one it used, \
             because that provider simply is not rotating.\n\nWhat is not secret — when it was \
             connected, the account, the granted scopes, the expiries — goes to a small JSON \
             file beside it. Nothing the page can read ever carries the token itself.",
        ),
        stage(
            "Say whose account it is",
            "One authenticated call, and it is allowed to fail.",
            "GitHub is asked at api.github.com/user, Google at its OpenID userinfo endpoint, and \
             the login or email is recorded beside the connection so the board can say \
             \"connected as\" rather than only \"connected\".\n\nOptional on purpose: a token \
             scoped for one API is not necessarily allowed to read the account that holds it, \
             and a sign-in that worked must not be reported as failed because a cosmetic call \
             did not. The connection is recorded either way, without a name.",
        ),
        stage(
            "Back to the page",
            "303 to the board the sign-in started from.",
            "The return address was taken from the start request's Origin and kept with the \
             pending flow — and only if that origin is one the API already trusts: the CORS list, \
             or the API's own address in a packaged install. Anything else falls back to a \
             relative path. Taking a return address from a request unchecked would make this \
             callback an open redirect.\n\nThe answer carries cache-control: no-store, and the \
             URL that carried the code is spent by the time the browser follows it.",
        ),
        stage(
            "Use it in a run",
            "The job reads a credential, as it always did.",
            "ctx.secret is synchronous and stays so: it reads the value the sign-in wrote and \
             knows nothing about OAuth. A job declares the credential by name, and the Jobs page \
             says whether it is set.\n\nWhere two routes are possible the job declares a choice \
             instead — credentialsAnyOf — and the three mail jobs do: an app password or the \
             Google token. Gmail will not take a token as a password, so the mail code sends it \
             through XOAUTH2 instead, and only when the sign-in granted \
             https://mail.google.com/. A token minted for Drive is a perfectly good token that \
             Gmail refuses, and being told that by name beats being told \"invalid \
             credentials\".",
        ),
        stage(
            "Renew",
            "Before a run, never inside one.",
            "The runner calls into the OAuth module with the credentials the job declared, \
             before it starts. A token more than five minutes from expiry is left alone. One \
             inside that margin is renewed with the refresh token — the same token endpoint, \
             grant_type=refresh_token — and the new values are stored exactly as a sign-in's \
             are.\n\nOnce, under a per-provider lock: two jobs starting at the same moment \
             would otherwise each spend the refresh token, and where the provider rotates it the \
             second would find its copy already invalid.\n\nA failed renewal on a token that \
             still has minutes left lets the run go ahead — it may well work, and stopping it \
             would turn a hiccup into an outage. A failed renewal on a token that is already \
             dead stops the run with the cause named, which beats the provider's 401 arriving \
             from somewhere inside the job.\n\nReplacing the token by hand on the credentials \
             board forgets what the sign-in recorded: the expiry and the account belonged to the \
             value that was there, and keeping them would have the page describing a token that \
             is gone.",
        ),
        stage(
            "Disconnect",
            "DELETE /api/oauth/:id — revoke there, forget here.",
            "The provider is told first, where it offers a way: GitHub takes the app's own \
             client ID and secret as Basic auth and deletes the grant; Google takes the token \
             alone at its revoke endpoint and kills the refresh token with it. The board reports \
             what the provider said.\n\nThen the token and any refresh token are removed from \
             this process and from the credentials file, and the stored connection is \
             forgotten.\n\nBoth halves matter, and in that order. Forgetting the token here \
             without revoking leaves a live grant nobody is watching; revoking without \
             forgetting leaves a credential that is set and refuses everything. If the provider \
             cannot be reached, the local half still happens and the panel says the token was \
             removed but not revoked — so you know there is a grant to kill on their site.",
        ),
    ]
}

const PANEL_WHAT: &str =
    "Signing rn in to a provider through the provider's own consent screen, so a job gets a \
     token without anyone creating one by hand. This is the OAuth authorization-code flow, with \
     a loopback redirect.\n\nIn one line: this backend mints a state and a PKCE verifier, the \
     browser goes to the provider and comes back to a loopback URL with a code, the backend \
     exchanges that code for a token, and the token lands in an ordinary credential a job \
     already reads.\n\nThe tabs above walk it, one per hop — the parameters each carries, what \
     is refused where, and what is stored. Start there when a sign-in stopped somewhere and you \
     need to know where.\n\nThe provider never connects to rn, no tunnel is involved, and the \
     hooks listener is untouched.";

const PANEL_WHY: &str =
    "Because the alternative is a personal access token made by hand: a page on the provider's \
     site, a choice of permissions, a value pasted into the credentials board, and a calendar \
     reminder for when it expires. A sign-in asks for the scopes a job needs and writes the \
     result where the job already reads it.\n\nThis page used to say the flow could not work here. \
     That was right about the hooks listener, whose safety is that it checks a signature on every \
     call, while an OAuth redirect is an unsigned GET. It was wrong about the flow. The redirect \
     never needed to reach this machine from outside, because the browser that follows it is \
     already here. RFC 8252 is the standard for exactly this: an app on the user's own machine \
     receiving the redirect on loopback.";

const PANEL_IF_WRONG: &str =
    "A failed sign-in lists, under Last sign-in, every hop that ran. The last line is where it \
     stopped. A state that did not match is refused before any exchange, with a short page saying \
     so. It is not recorded here, because rn did not start that sign-in and a page that could \
     write failures into this panel by opening a URL would be a way to mislead you.\n\nState is \
     held in memory, so restarting the backend while the provider's consent screen is open makes \
     the return refused. Start again.\n\nUnder Deno, github.com and api.github.com must be in the \
     outbound grant, or the exchange is refused by the runtime and the error says which host.";

const CLIENT_ID_WHAT: &str =
    "The public identifier of the app you register with the provider: an OAuth App under GitHub \
     → Settings → Developer settings. It goes into the URL the browser is sent to, so it is not \
     secret. It is stored as a credential anyway, so the app's two halves live in one file and \
     are set the same way.";

const CLIENT_ID_WHY: &str =
    "rn does not ship a registered app of its own. An app shipped inside an installed program \
     cannot keep its secret, and every install would share one identity at the provider. So each \
     install registers its own. It takes a minute, and it means the grant on the provider's \
     side is yours to see and revoke by name.";

const CLIENT_ID_IF_WRONG: &str =
    "A wrong ID fails on the provider's page rather than here, usually as a 404 or \"application \
     not found\". Nothing can warn you sooner, since checking would mean asking the provider. \
     Copy it from the app's settings page rather than retyping it.";

const CLIENT_SECRET_WHAT: &str =
    "The app's password, used once per sign-in: when the backend exchanges the code for a token. \
     It goes from this backend to the provider's token endpoint and nowhere else. It is never in \
     a browser URL and never on this page.";

const CLIENT_SECRET_WHY: &str =
    "The provider needs to know the exchange comes from the app the user approved, not from \
     someone else who got hold of the code. For an app on a user's machine the secret is weak \
     proof of that, since anything that can read the credentials file can read it. PKCE is the \
     real protection: the code is useless without a verifier that existed only in this process's \
     memory.";

const CLIENT_SECRET_IF_WRONG: &str =
    "A wrong secret passes the consent screen and then fails the exchange. GitHub answers \
     incorrect_client_credentials, and the Last sign-in lines show it on the exchange hop. \
     Revoking the token on disconnect uses the secret too, so a wrong one leaves the grant alive \
     at GitHub, and the disconnect message says so.";

const CALLBACK_WHAT: &str =
    "What to put in the app's \"Authorization callback URL\" field when you register it. \
     Loopback, with no port.\n\nThe port is left out on purpose. GitHub matches a loopback \
     callback on its host and path and lets the port vary, so one registered app serves every \
     backend on this machine: 3010, and each worktree's own port.";

const CALLBACK_WHY: &str =
    "The provider only sends the browser to a callback the app registered. That is what stops \
     someone making a sign-in link for your app that sends the code to their own server. \
     Loopback is safe to register because it means \"this machine\" wherever it is followed: \
     the code can only reach the backend on the computer whose browser followed the link.";

const CALLBACK_IF_WRONG: &str =
    "A mismatch fails on GitHub's page as \"The redirect_uri is not associated with this \
     application\", before you are asked to approve anything. The usual causes are localhost \
     registered where 127.0.0.1 is sent (they are different hosts to GitHub), https where http \
     is sent, or a path that does not match.";

const REDIRECT_WHAT: &str =
    "The exact redirect URI this backend puts in the sign-in URL: the registered callback plus \
     the port this backend is listening on. The provider sends the browser here with the \
     code.";

const REDIRECT_WHY: &str =
    "It names the API listener, not the hooks listener, and that is the design. The API is bound \
     to loopback and is where this page already talks. A request can reach it only from this \
     machine, which is where the browser following the redirect is. The hooks listener is the \
     one a tunnel points at, and it serves one signed route and nothing else.";

const REDIRECT_IF_WRONG: &str =
    "If the API is bound to a specific network address, no loopback redirect is possible and \
     this row says so. The provider would be sending a code over plain http to an address on a \
     network, which is refused rather than attempted. A wildcard bind still answers on 127.0.0.1, \
     so that is what gets sent.";

const SCOPES_WHAT: &str =
    "What the token will be allowed to do, asked for on the consent screen. GitHub OAuth Apps use \
     classic scopes: read:repo_hook reads repositories' webhooks and their delivery logs, which is \
     what watch-deliveries needs. repo is full access to private repositories. Empty means public \
     data only.";

const SCOPES_WHY: &str =
    "Ask for the least a job needs. The token sits in a plaintext file (see docs/sec.md), and \
     what it can do is what anyone who reads that file can do. The consent screen shows the \
     scopes too, so a person approving it can see what they are granting.";

const SCOPES_IF_WRONG: &str =
    "Too few scopes and the sign-in succeeds, then a job fails with 403 or 404 on the first \
     request that needed more. GitHub answers 404 rather than 403 for a private repository the \
     token cannot see. The Granted row shows what was actually granted, which can be less than \
     was asked. A GitHub App ignores scopes and uses the permissions set on the app instead.";

const TOKEN_WHAT: &str =
    "The credential the access token is written into: the same one a job already reads with \
     ctx.secret. For GitHub that is githubToken, which watch-deliveries declares. Signing in \
     is one more way to fill that credential, not a second store. The job cannot tell, and \
     does not need to.";

const TOKEN_WHY: &str =
    "So no job changes. The value takes the same path as one typed into the credentials board: \
     into the running process first, which is what starts it being scrubbed out of run records, \
     then into ~/.config/rn/credentials. It works immediately, with no restart.\n\nWhat is not \
     secret goes to oauth.json beside it: the account, the scopes granted, and when the token \
     dies. That lets this page and the tokens board on Monitor → Connection show them without \
     the token ever crossing to the browser.";

const TOKEN_IF_WRONG: &str =
    "Signing in replaces whatever githubToken held, a personal token included. Setting \
     githubToken by hand on the credentials board afterwards does the reverse: this panel forgets \
     the sign-in, and its expiry and refresh token with it, because they no longer describe the \
     value. Editing the file directly is not seen that way, so the account shown here can go \
     stale until the next sign-in or disconnect.";

const GRANTED_WHAT: &str =
    "The scopes the provider actually granted, and whether rn holds a refresh token.\n\nA GitHub \
     OAuth App token does not expire and comes with no refresh token. It lasts until it is \
     revoked, or until it goes unused for a year. A GitHub App with expiring tokens gives an \
     eight-hour token plus a six-month refresh token, and this row shows both clocks.";

const GRANTED_WHY: &str =
    "Renewal happens before a run, not during one. When a job that reads this token is about \
     to start and the token has less than five minutes left, the runner renews it first. It does \
     that once, even if several runs start at the same moment, because two renewals racing would \
     each spend the refresh token and the second would find it already used.";

const GRANTED_IF_WRONG: &str =
    "A token that has already expired and cannot be renewed stops the run before it starts. The \
     run record names the credential, when it expired, and why renewal failed, instead of the \
     ordinary 401 the first request would have got. A failed renewal of a token with a few \
     minutes left is logged, and the run goes ahead on the old token.";

const SIGNIN_WHAT: &str =
    "Starts the flow described in this panel's own info. This tab goes to the provider and \
     comes back here when you approve or decline. Disconnect removes the token from rn and asks \
     the provider to revoke it.";

const SIGNIN_WHY: &str =
    "Disconnecting does two separate things, and both are reported. Removing the token here \
     stops rn using it. Revoking it at the provider stops it working anywhere, including a copy \
     that left this machine. The first never waits on the second, so a provider being down does \
     not leave a token in use. When revocation fails, the message says so and points you at the \
     provider's settings page.";

const SIGNIN_IF_WRONG: &str =
    "The button stays inactive until the client ID and secret are set and a loopback redirect is \
     possible. If you approve on GitHub and land on a page saying rn did not start this sign-in, \
     the backend restarted or ten minutes passed in between. Press the button again.";
