use crate::components::header::Header;
use crate::components::SubNav;
use crate::pages::{
    Config, ConfigConnection, ConfigJobs, ConfigMail, Home, MonitorConnection, MonitorJobs, MonitorLinks,
    MonitorLinksSend, MonitorRuntime, PageNotFound,
};
use dioxus::prelude::*;
use dioxus_router::{Outlet, Routable, Router};

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Layout)]
        #[route("/")]
        Home {},
        // Named for the job, not for one runtime: which runtime this page
        // reports on is a setting under Config, and a URL saying "node"
        // while the page says "bun runtime" is the app contradicting
        // itself. The old path is kept as a redirect — it was linkable.
        #[redirect("/monitor/node", || Route::MonitorRuntime {})]
        #[route("/monitor/runtime")]
        MonitorRuntime {},
        #[route("/monitor/jobs")]
        MonitorJobs {},
        // The measured counterpart of Config → Connection, the same way
        // MonitorRuntime is the counterpart of Config → Runtime.
        #[route("/monitor/connection")]
        MonitorConnection {},
        // Under Monitor rather than Config: a tracked link is something that
        // happened, not something set. What can be configured about it — where
        // links point, how long identity is kept — is environment, and the page
        // reports it rather than editing it.
        #[route("/monitor/links")]
        MonitorLinks {},
        // The arrivals on one send, addressable. A page whose whole job is
        // evidence has to be linkable: a detail that lives only in a signal
        // cannot be reloaded, cannot be sent to the person asking about the
        // number, and cannot be looked at without clicking a button first.
        #[route("/monitor/links/:id")]
        MonitorLinksSend { id: String },
        #[route("/config")]
        Config {},
        // What is listening, who may talk to it, and what it may reach. Its
        // own page rather than a board on Config, because the three facts are
        // read together and none of them is a runtime parameter.
        #[route("/config/connection")]
        ConfigConnection {},
        // Mail rules: which mailboxes are watched and what counts in each. Its
        // own page rather than a board on Config, because a rule is a record
        // you make and delete rather than a value you set — the same reason
        // webhooks are not a settings row.
        #[route("/config/mail")]
        ConfigMail {},
        // A second page under Config, not a tab inside the first: what a job
        // is configured to do is declared in code, and mixing it into a board
        // of editable settings would imply it is one.
        #[route("/config/jobs")]
        ConfigJobs {},
        #[route("/:..segments")]
        PageNotFound { segments: Vec<String> },
}

#[component]
pub fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/styling/output.css") }

        Router::<Route> {}
    }
}

#[component]
fn Layout() -> Element {
    // Mount-time: add `dark` class to <html> once. Required because
    // assets/styling/index.css declares `@custom-variant dark (&:where(.dark &));`
    // — any `dark:` Tailwind variant from third-party / daisyUI resolves through
    // the class. The app is dark-only, so the class is always on.
    use_effect(move || {
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document() {
                if let Some(html) = document.document_element() {
                    let _ = html.class_list().add_1("dark");
                }
            }
        }
    });

    rsx! {
        div { class: "min-h-screen bg-gray-900 text-white",

            Header {}

            // Section bar: the pages under whichever header link you are in.
            SubNav {}

            main {
                Outlet::<Route> {}
            }
        }
    }
}
