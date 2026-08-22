use dioxus::prelude::*;

/// Info button + panel. The vehicle for the educational requirement in
/// CLAUDE.md: every control the user could ask "what is this?" about gets one.
///
/// Style values are fixed here so no page reinvents them.
pub const INFO_BUTTON_CLASS: &str =
    "w-6 h-6 min-w-6 min-h-6 shrink-0 rounded flex items-center justify-center cursor-pointer hover:opacity-80";
pub const INFO_BUTTON_STYLE: &str =
    "background-color: #7C2A02; border: 1px solid #7C2A02;"; // Rust brand color
pub const INFO_ICON_SVG_CLASS: &str = "w-5 h-5 text-white";

#[component]
pub fn InfoIcon() -> Element {
    rsx! {
        svg {
            class: INFO_ICON_SVG_CLASS,
            view_box: "0 0 20 20",
            fill: "none",
            stroke: "white",
            circle { cx: "10", cy: "10", r: "9", stroke_width: "1.5" }
            line { x1: "10", y1: "8", x2: "10", y2: "14", stroke_width: "1.5" }
            circle { cx: "10", cy: "6.3", r: "1", fill: "white", stroke: "none" }
        }
    }
}

/// A button that opens an explanation panel.
///
/// `what` / `why` / `if_wrong` mirror the three fields every parameter carries
/// in the backend registry — the panel is filled from data, not hand-written
/// per control.
#[component]
pub fn InfoButton(
    title: String,
    what: String,
    why: String,
    if_wrong: String,
) -> Element {
    let mut open = use_signal(|| false);

    rsx! {
        button {
            class: INFO_BUTTON_CLASS,
            style: INFO_BUTTON_STYLE,
            title: "What this setting does",
            onclick: move |_| open.set(true),
            InfoIcon {}
        }

        if open() {
            div {
                class: "fixed inset-0 flex items-center justify-center bg-black/70 p-4",
                style: "z-index: 1110;",
                onclick: move |_| open.set(false),
                div {
                    class: "bg-gray-900 border border-gray-700 rounded-lg p-6 w-[90vw] max-w-3xl max-h-[95vh] overflow-y-auto shadow-xl text-sm space-y-4",
                    onclick: move |evt| evt.stop_propagation(),

                    div { class: "flex items-center justify-between",
                        h2 { class: "text-xl font-bold text-gray-100", "{title}" }
                        button {
                            class: "text-gray-400 hover:text-gray-200 text-xl font-bold cursor-pointer",
                            onclick: move |_| open.set(false),
                            "×"
                        }
                    }

                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "What it does" }
                        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line", "{what}" }
                    }
                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "Why you would change it" }
                        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line", "{why}" }
                    }
                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "If it's wrong" }
                        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line", "{if_wrong}" }
                    }
                }
            }
        }
    }
}
