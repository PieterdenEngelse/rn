use crate::app::Route;
use crate::components::sub_nav::in_section;
use crate::components::StatusLight;
use dioxus::prelude::*;
use dioxus_router::{use_route, Link};

/// Nav link colors — active vs idle.
const NAV_ACTIVE: &str = "#7C2A02";
const NAV_IDLE: &str = "white";

/// Brand color used for the app title.
const BRAND: &str = "#026B7C";

const NAV_LINK_CLASS: &str = "py-2 px-3 rounded-lg transition-colors font-medium";

#[component]
pub fn Header() -> Element {
    let mut menu_open = use_signal(|| false);
    let current_route = use_route::<Route>();

    // gray-700 (#374151) sits ~20% of the way from gray-900 (#111827) to white —
    // a lighter grey than the page shell, still on the Tailwind palette.
    let header_bg = "bg-gray-700";

    let monitor_color = if in_section(&current_route, "Monitor") {
        NAV_ACTIVE
    } else {
        NAV_IDLE
    };
    let config_color = if in_section(&current_route, "Config") {
        NAV_ACTIVE
    } else {
        NAV_IDLE
    };

    rsx! {
        header {
            class: "sticky top-0 shadow-md py-0 px-0.5 transition-colors {header_bg} flex items-center relative",
            style: "z-index: 60;",

            // Title — flex-1 center column, truncates on small screens
            div { class: "flex-1 min-w-0 flex justify-center items-center gap-2",
                Link {
                    to: Route::Home {},
                    class: "font-medium truncate",
                    style: "font-family: ui-sans-serif, system-ui, sans-serif; font-size: 0.975rem; color: {BRAND};",
                    "rn"
                }
                StatusLight {}
            }

            div { class: "flex-shrink-0 flex justify-end items-center",

                nav {
                    class: "hidden md:flex items-center gap-[0.1875rem] text-sm",
                    style: "font-family: ui-sans-serif, system-ui, sans-serif;",

                    Link {
                        to: Route::MonitorRuntime {},
                        class: NAV_LINK_CLASS,
                        style: format!("color: {};", monitor_color),
                        "Monitor"
                    }
                    Link {
                        to: Route::Config {},
                        class: NAV_LINK_CLASS,
                        style: format!("color: {};", config_color),
                        "Config"
                    }
                }
                button {
                    class: "md:hidden p-2 text-2xl",
                    onclick: move |_| menu_open.set(!menu_open()),
                    "☰"
                }
            }

            if menu_open() {
                div { class: "md:hidden absolute top-full right-0 w-40 bg-gray-900 shadow-md p-4 flex flex-col gap-4",
                    Link {
                        to: Route::MonitorRuntime {},
                        class: "text-teal-100 hover:text-white transition-colors",
                        onclick: move |_| menu_open.set(false),
                        "Monitor"
                    }
                    Link {
                        to: Route::Config {},
                        class: "text-teal-100 hover:text-white transition-colors",
                        onclick: move |_| menu_open.set(false),
                        "Config"
                    }
                }
            }
        }
    }
}
