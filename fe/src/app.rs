use crate::components::header::Header;
use crate::components::SubNav;
use crate::pages::{
    Config, Home, Monitor, MonitorJobs, MonitorNode, PageNotFound,
};
use dioxus::prelude::*;
use dioxus_router::{Outlet, Routable, Router};

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Layout)]
        #[route("/")]
        Home {},
        #[route("/monitor")]
        Monitor {},
        #[route("/monitor/node")]
        MonitorNode {},
        #[route("/monitor/jobs")]
        MonitorJobs {},
        #[route("/config")]
        Config {},
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
