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
    disconnect_oauth, fetch_oauth, save_credential, start_oauth, OAuthAttempt, OAuthConnection,
    OAuthProvider,
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

const PANEL_WHAT: &str =
    "Signing rn in to a provider through the provider's own consent screen, so a job gets a \
     token without anyone creating one by hand. This is the OAuth authorization-code flow, with \
     a loopback redirect.\n\nFive hops, in order:\n\n1. Pressing the button asks this backend to \
     start. It makes two random values: state, which it remembers for ten minutes, and a PKCE \
     verifier, which never leaves it. It answers with the provider's URL, carrying the state and \
     a hash of the verifier.\n2. This tab goes to the provider. You sign in there and approve.\n3. \
     The provider sends this tab back to http://127.0.0.1:<API port>/api/oauth/<provider>/callback \
     with a one-time code. That is the backend on this machine; the browser is the only thing \
     that travels.\n4. The backend checks the state against the one it issued and spends it, then \
     POSTs the code, the verifier and the app's secret to the provider. That is an outbound \
     request, the direction that always worked.\n5. The token that comes back is written into an \
     ordinary credential, and this tab is sent back here.\n\nThe provider never connects to rn, \
     no tunnel is involved, and the hooks listener is untouched.";

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
