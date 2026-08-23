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
    /// Where the runtime changed, as a fraction from the left, and what was
    /// running before it. Ruled in amber rather than shaded: the process-start
    /// shade already says "different run", and this says the stronger thing —
    /// the numbers left of the rule were measured by something else and are not
    /// comparable with the ones right of it.
    #[props(default = None)]
    runtime_change: Option<(f64, String)>,
    /// Stretch to the height of whatever the plot sits beside, instead of the
    /// fixed `height`. The viewBox is unchanged, so the curve simply resolves
    /// over more pixels — the same data, read against a taller y axis.
    #[props(default = false)]
    fill_height: bool,
) -> Element {
    let width = 240.0_f64;
    let h = height as f64;

    let peak = series
        .iter()
        .flat_map(|s| s.points.iter().filter_map(|p| *p))
        .fold(0.0_f64, f64::max)
        .max(reference.unwrap_or(0.0) * 0.0) // reference must not squash the data
        .max(f64::MIN_POSITIVE);

    // A little headroom so the peak is not glued to the top edge.
    let scale = peak * 1.15;

    // Two points somewhere in the series, not two slots: a window that is
    // mostly gaps has a length without having a line.
    let longest = series
        .iter()
        .map(|s| s.points.iter().filter(|p| p.is_some()).count())
        .max()
        .unwrap_or(0);
    if longest < 2 {
        return rsx! {
            div { class: "text-gray-400 text-xs", "collecting…" }
        };
    }

    /// The runs of consecutive readings in a series, as polyline paths.
    ///
    /// A missing point is a gap rather than a zero — the runtime that took the
    /// sample did not measure that figure — so the line stops and starts again
    /// instead of diving to the axis and back. Each run keeps its original
    /// index, so x still means "when", and a series that measured only its
    /// first half draws over only its first half.
    ///
    /// A run of one is dropped: a polyline needs two points, and a lone reading
    /// between two gaps would render as nothing anyway.
    fn to_paths(points: &[Option<f64>], width: f64, h: f64, scale: f64) -> Vec<String> {
        let n = points.len().max(2) as f64;
        let mut paths = Vec::new();
        let mut run: Vec<String> = Vec::new();
        for (i, v) in points.iter().enumerate() {
            match v {
                Some(v) => {
                    let x = (i as f64 / (n - 1.0)) * width;
                    let y = h - (v / scale).clamp(0.0, 1.0) * h;
                    run.push(format!("{x:.1},{y:.1}"));
                }
                None => {
                    if run.len() > 1 {
                        paths.push(run.join(" "));
                    }
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            paths.push(run.join(" "));
        }
        paths
    }

    rsx! {
        div {
            // flex-1 as well as h-full: h-full is a percentage and resolves only
            // if every ancestor has a definite height, which leaves the plot at
            // its content height and the board with slack under it. flex-1 asks
            // the parent column for the leftover directly, which is what the
            // board actually has to give.
            class: if fill_height { "flex flex-col gap-1 h-full flex-1 min-h-0" } else { "flex flex-col gap-1" },
            svg {
                class: if fill_height { "flex-1 min-h-0" } else { "" },
                width: "100%",
                height: if fill_height { "100%".to_string() } else { height.to_string() },
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
                    for path in to_paths(&s.points, width, h, scale) {
                        polyline {
                            points: "{path}",
                            fill: "none",
                            stroke: "{s.color}",
                            stroke_width: "1.5",
                            stroke_linejoin: "round",
                        }
                    }
                }

                // The runtime boundary, drawn last so it sits over the data
                // rather than under it: it is a caveat on the curve, and has to
                // stay visible where the curve is densest.
                if let Some((at, _)) = runtime_change.as_ref() {
                    {
                        let x = at.clamp(0.0, 1.0) * width;
                        rsx! {
                            line {
                                x1: "{x:.1}", y1: "0", x2: "{x:.1}", y2: "{h}",
                                stroke: "#f59e0b", stroke_width: "1",
                                stroke_dasharray: "4 2",
                            }
                        }
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
                    // Named, not just ruled. A dashed line on its own reads as
                    // another threshold; the point is which runtime measured
                    // the stretch to its left.
                    if let Some((_, previous)) = runtime_change.as_ref() {
                        div { class: "flex items-center gap-1 text-amber-400",
                            span {
                                class: "inline-block w-2 h-0 border-t border-dashed",
                                style: "border-color: #f59e0b;",
                            }
                            span { "← {previous}" }
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
    /// One slot per sample, in time order. `None` is a gap — the runtime that
    /// took that sample does not measure this figure — and is drawn as a break
    /// in the line rather than as a zero.
    pub points: Vec<Option<f64>>,
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
