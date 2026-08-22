use dioxus::prelude::*;

/// A small time-series plot, drawn as inline SVG.
///
/// No chart library: a polyline over a fixed viewBox is the whole requirement,
/// and a dependency for it would be larger than the code.
///
/// Series share one vertical scale so they can be read against each other —
/// scaling each to its own maximum would make a flat line and a spike look
/// identical.
#[component]
pub fn Sparkline(
    series: Vec<Series>,
    /// Drawn as a dashed rule, for a ceiling like the heap limit.
    #[props(default = None)]
    reference: Option<f64>,
    #[props(default = 40)] height: u32,
    /// Appended to the max label, e.g. "MB".
    #[props(default = String::new())]
    unit: String,
    /// Fraction of the drawn series, from the left, recorded before this
    /// process started. Shaded and ruled: the numbers are real and restored
    /// from disk, but they came from a different process, and a continuous
    /// curve across a restart implies a continuity that did not happen.
    #[props(default = 0.0)]
    before_start: f64,
) -> Element {
    let width = 240.0_f64;
    let h = height as f64;

    let peak = series
        .iter()
        .flat_map(|s| s.points.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(reference.unwrap_or(0.0) * 0.0) // reference must not squash the data
        .max(f64::MIN_POSITIVE);

    // A little headroom so the peak is not glued to the top edge.
    let scale = peak * 1.15;

    let longest = series.iter().map(|s| s.points.len()).max().unwrap_or(0);
    if longest < 2 {
        return rsx! {
            div { class: "text-gray-500 text-xs", "collecting…" }
        };
    }

    let to_path = |points: &[f64]| -> String {
        let n = points.len().max(2) as f64;
        points
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let x = (i as f64 / (n - 1.0)) * width;
                let y = h - (v / scale).clamp(0.0, 1.0) * h;
                format!("{x:.1},{y:.1}")
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    rsx! {
        div { class: "flex flex-col gap-1",
            svg {
                width: "100%",
                height: "{height}",
                view_box: "0 0 {width} {h}",
                preserve_aspect_ratio: "none",
                role: "img",

                // The stretch before this process existed, shaded and closed
                // with a rule at the moment it started. Drawn first so the data
                // sits on top of it.
                if before_start > 0.0 {
                    {
                        let w = (before_start.clamp(0.0, 1.0)) * width;
                        rsx! {
                            rect {
                                x: "0", y: "0", width: "{w:.1}", height: "{h}",
                                fill: "#374151", fill_opacity: "0.45",
                            }
                            line {
                                x1: "{w:.1}", y1: "0", x2: "{w:.1}", y2: "{h}",
                                stroke: "#9ca3af", stroke_width: "1",
                                stroke_dasharray: "2 2",
                            }
                        }
                    }
                }

                // Baseline
                line {
                    x1: "0", y1: "{h}", x2: "{width}", y2: "{h}",
                    stroke: "#4b5563", stroke_width: "1",
                }

                if let Some(r) = reference {
                    {
                        let y = h - (r / scale).clamp(0.0, 1.0) * h;
                        rsx! {
                            line {
                                x1: "0", y1: "{y:.1}", x2: "{width}", y2: "{y:.1}",
                                stroke: "#6b7280", stroke_width: "1",
                                stroke_dasharray: "3 3",
                            }
                        }
                    }
                }

                for s in series.iter() {
                    polyline {
                        points: to_path(&s.points),
                        fill: "none",
                        stroke: "{s.color}",
                        stroke_width: "1.5",
                        stroke_linejoin: "round",
                    }
                }
            }

            div { class: "flex items-center justify-between text-[10px] text-gray-400",
                div { class: "flex items-center gap-3",
                    for s in series.iter() {
                        div { class: "flex items-center gap-1",
                            span {
                                class: "inline-block w-2 h-2 rounded-sm",
                                style: "background-color: {s.color};",
                            }
                            span { "{s.label}" }
                        }
                    }
                }
                span { "peak {format_num(peak)}{unit}" }
            }
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Series {
    pub label: String,
    pub color: String,
    pub points: Vec<f64>,
}

fn format_num(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}
