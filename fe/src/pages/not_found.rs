use crate::app::Route;
use dioxus::prelude::*;
use dioxus_router::Link;

#[component]
pub fn PageNotFound(#[props(default = vec![])] segments: Vec<String>) -> Element {
    rsx! {
        div { class: "p-8 text-center",
            h1 { class: "text-3xl font-bold text-red-400", "404 – Page Not Found" }
            p { class: "mt-2 text-gray-300", "Sorry, the page you're looking for doesn't exist." }
            p { class: "mt-2 text-sm text-gray-400", "Attempted path: /{segments.join(\"/\")}" }
            Link {
                to: Route::Home {},
                class: "mt-4 inline-block px-4 py-2 bg-gray-600 text-white rounded hover:bg-gray-700",
                "Return Home"
            }
        }
    }
}
