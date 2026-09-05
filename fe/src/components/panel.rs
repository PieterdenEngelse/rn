use dioxus::prelude::*;

/// Panel — the standard surface for a group of content.
#[component]
pub fn Panel(
    #[props(default = None)] title: Option<String>,
    #[props(default = None)] subtitle: Option<String>,
    /// Optional control rendered beside the title — an InfoButton explaining
    /// what the whole panel is about, as opposed to any one row inside it.
    #[props(default = None)] info: Option<Element>,
    /// Controls belonging to the panel as a whole — placed on the title line
    /// rather than as a strip beneath it that costs a row of vertical space.
    ///
    /// At the row's **far right**, outside the `header_class` box, because that
    /// box is sometimes sized to align its info button with a column of buttons
    /// further down the panel. A control added inside it steals the width the
    /// title and subtitle were fitted to, and the subtitle wraps — which is
    /// exactly what putting the dry-run switch on Config's Runtime tile did to
    /// "which runtime runs the app".
    #[props(default = None)] actions: Option<Element>,
    /// Extra classes for the title row. Lets a page size that row so its info
    /// button lines up with a column of them further down the panel; without
    /// it the button lands wherever the title and subtitle happen to end.
    #[props(default = String::new())] header_class: String,
    /// Extra classes for the panel's own box — how it sits in the layout
    /// around it, not what is inside it. A panel stacked in the page column
    /// needs none; one placed beside another has to say how the two share the
    /// row, and only the page laying them out knows that.
    #[props(default = String::new())] class: String,
    children: Element,
) -> Element {
    rsx! {
        div { class: "bg-gray-800 border border-gray-700 rounded-lg p-4 shadow {class}",
            // Drawn for a panel with no title too, when it still has controls or
            // an explanation to put on that line — dropping the heading must not
            // take a Pause button and an uptime with it.
            if title.is_some() || subtitle.is_some() || info.is_some() || actions.is_some() {
                div { class: "flex items-center justify-between mb-3",
                    div { class: "flex items-center gap-3 {header_class}",
                        if let Some(title) = title {
                            h3 { class: "text-sm font-semibold text-gray-200", "{title}" }
                        }
                        if let Some(subtitle) = subtitle {
                            span { class: "text-[10px] text-gray-400", "{subtitle}" }
                        }
                        if let Some(info) = info {
                            {info}
                        }
                    }
                    if let Some(actions) = actions {
                        {actions}
                    }
                }
            }
            div { class: "text-gray-100 text-xs space-y-2", {children} }
        }
    }
}
