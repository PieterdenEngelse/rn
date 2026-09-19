use crate::app::Route;
use dioxus::prelude::*;
use dioxus_router::{use_route, Link};

/// The section bar under the header.
///
/// Adding a section is one entry in [`sections`] — the bar, the highlighting
/// and the "which section am I in" logic all follow from it. Nothing needs a
/// prop passed down from each page, so a new page cannot forget to declare
/// itself.
#[component]
pub fn SubNav() -> Element {
    let current = use_route::<Route>();
    let Some(section) = section_for(&current) else {
        // Home and 404 have no children — no bar rather than an empty one.
        return rsx! {};
    };

    rsx! {
        div {
            class: "sticky bg-gray-800 border-b border-gray-700 px-4 py-2",
            style: "top: 2rem; z-index: 50;",
            nav { class: "flex flex-wrap items-center gap-4 text-sm",
                span { class: "text-gray-500", "{section.label}" }
                span { class: "text-gray-600", "/" }
                for item in section.items.iter() {
                    {
                        let is_active = item.route == current;
                        rsx! {
                            Link {
                                to: item.route.clone(),
                                class: if is_active {
                                    "text-white border-b-2 border-white pb-1"
                                } else {
                                    "text-gray-400 hover:text-white transition-colors"
                                },
                                "{item.label}"
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct SubNavItem {
    pub label: &'static str,
    pub route: Route,
}

pub struct Section {
    pub label: &'static str,
    pub items: Vec<SubNavItem>,
}

/// Every section and its pages. **This is the list to extend.**
fn sections() -> Vec<Section> {
    vec![
        Section {
            label: "Monitor",
            items: vec![
                                SubNavItem { label: "Runtime", route: Route::MonitorRuntime {} },
                SubNavItem { label: "Jobs", route: Route::MonitorJobs {} },
                SubNavItem { label: "Connection", route: Route::MonitorConnection {} },
                SubNavItem { label: "Mail", route: Route::MonitorMail {} },
                SubNavItem { label: "Links", route: Route::MonitorLinks {} },
                SubNavItem { label: "Webhooks", route: Route::MonitorWebhooks {} },
            ],
        },
        Section {
            label: "Config",
            // Process folded into Runtime: the Restart board sits beside the
            // runtime it restarts, which is where it is actually useful.
            items: vec![
                SubNavItem { label: "Runtime", route: Route::Config {} },
                SubNavItem { label: "Connection", route: Route::ConfigConnection {} },
                SubNavItem { label: "Jobs", route: Route::ConfigJobs {} },
                SubNavItem { label: "Mail", route: Route::ConfigMail {} },
                SubNavItem { label: "Watching", route: Route::ConfigWatching {} },
                SubNavItem { label: "Webhooks", route: Route::ConfigWebhooks {} },
            ],
        },
    ]
}

/// Which section owns this route, if any.
fn section_for(current: &Route) -> Option<Section> {
    sections()
        .into_iter()
        .find(|s| s.items.iter().any(|i| i.route == *current))
}

/// True when the route belongs to this section — used by the header to keep a
/// top-level link highlighted while you are on one of its children.
pub fn in_section(current: &Route, label: &str) -> bool {
    section_for(current).map(|s| s.label == label).unwrap_or(false)
}
