use crate::components::ProcessPanel;
use dioxus::prelude::*;

/// Config → Process. Start, stop, restart, and what is running.
#[component]
pub fn ConfigProcess() -> Element {
    // The panel refreshes itself; nothing on this page depends on a reload
    // counter, so it gets its own.
    let reload = use_signal(|| 0u32);

    rsx! {
        div { class: "p-6 max-w-5xl mx-auto space-y-4",
            ProcessPanel { reload }
        }
    }
}
