use dioxus::prelude::*;

/// Panel — the standard surface for a group of content.
#[component]
pub fn Panel(
    #[props(default = None)] title: Option<String>,
    #[props(default = None)] subtitle: Option<String>,
    /// Optional control rendered beside the title — an InfoButton explaining
    /// what the whole panel is about, as opposed to any one row inside it.
    #[props(default = None)] info: Option<Element>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "bg-gray-800 border border-gray-700 rounded-lg p-4 shadow",
            if let Some(title) = title {
                div { class: "flex items-center justify-between mb-3",
                    div { class: "flex items-center gap-3",
                        h3 { class: "text-sm font-semibold text-gray-200", "{title}" }
                        if let Some(subtitle) = subtitle {
                            span { class: "text-[10px] text-gray-400", "{subtitle}" }
                        }
                        if let Some(info) = info {
                            {info}
                        }
                    }
                }
            }
            div { class: "text-gray-100 text-xs space-y-2", {children} }
        }
    }
}
