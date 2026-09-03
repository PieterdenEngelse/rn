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
    /// Readings to show beside the plot, in fixed slots spread evenly down it.
    ///
    /// They used to be pinned at the height of their own value and pushed apart
    /// where two of them collided, so that a reading sat on the line it named.
    /// The cost was a column that never held still: a value crossing another
    /// moved a block three lines tall, and the eye had to find each reading
    /// again on every sample. A legend that moves is harder to read than one
    /// that is merely near its line, so position is now a function of how many
    /// labels there are and nothing else — the same reading is in the same
    /// place on every render, whatever the data does. The swatch says which
    /// curve each one names.
    #[props(default = vec![])]
    side_labels: Vec<SideLabel>,
    /// Whether to print the peak in the legend row. A plot whose readings are
    /// pinned beside it says it there instead, under the series the peak
    /// belongs to — in the legend it named no series in particular.
    #[props(default = true)]
    show_peak: bool,
    /// Whether to draw the legend row at all. A plot whose series are named
    /// beside their own lines has nothing left to put in one, and the row is
    /// pure height. The runtime-change note lives there too, so the row still
    /// appears when there is one to show.
    #[props(default = true)]
    show_legend: bool,
    /// A CSS length to shift the label gutter by, negative to lift it. The
    /// slots divide the plot's height evenly, which is the right default — but
    /// the block around each reading is taller than one line, so a board may
    /// want the column sitting a little higher than dead centre.
    #[props(default = None)]
    labels_shift: Option<String>,
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

    // Where each reading sits: slot i of n, evenly down the plot, and nothing
    // else feeds into it.
    //
    // This is the whole of the fix. Placing a label at the height of its value
    // put it on the line it named, which reads well in a screenshot and badly
    // in a live plot: the readings sample every two seconds, so a block three
    // lines tall slid down the gutter while you were reading it, and two series
    // crossing swapped the order of the column outright. The collision pass
    // that kept them from overprinting made it worse rather than better, since
    // a label then moved for a reason that had nothing to do with its own
    // value.
    //
    // Fixed slots give up the one thing value-placement bought — a reading is
    // now merely beside its curve rather than on it — and the swatch already
    // covers that, being the same square the legend row uses. What they buy is
    // that the memory reading is in the same place every time you look at it.
    let label_tops: Vec<String> = {
        let n = side_labels.len().max(1) as f64;
        (0..side_labels.len())
            .map(|i| format!("{:.2}%", ((i as f64 + 0.5) / n) * 100.0))
            .collect()
    };

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
            class: if fill_height { "flex flex-col gap-1 flex-1 min-h-0" } else { "flex flex-col gap-1" },
            // The plot is positioned inside its box rather than being the
            // flex item itself. An svg as a flex item is sized differently by
            // each engine — Chromium flexes it, Firefox falls back to its
            // intrinsic height — so the box takes the leftover and the plot
            // simply fills the box, which both agree on.
            // Plot and label gutter side by side, both stretched by the row, so
            // the gutter is exactly as tall as the plot — which is what makes a
            // percentage down the gutter mean the same thing as a percentage
            // down the plot. Positioning the labels off the plot's right edge
            // instead took them out of the flow: they claimed no width, the
            // board shrank to fit what was left, and the labels were drawn over
            // the board beside it.
            div { class: if fill_height { "flex gap-4 flex-1 min-h-0" } else { "flex gap-4" },
            // `height` is a floor here, not a target. `flex-1` alone asks the
            // parent column for its leftover, and a board whose parent has no
            // height of its own has none to give: the plot resolves to zero and
            // the board draws a legend under a title with nothing between them.
            // That is what the History boards did — the svg was in the page,
            // its polylines carried real coordinates, and the box holding them
            // was a few pixels tall. So the box may grow past `height` when
            // there is room, and may not shrink below it when there is not,
            // which also makes the fixed and the stretched plot agree on a
            // minimum instead of differing by everything.
            div {
                class: if fill_height { "relative flex-1" } else { "relative flex-1" },
                style: if fill_height { "min-height: {height}px" } else { "" },
                svg {
                    class: if fill_height { "absolute inset-0 w-full h-full" } else { "" },
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
                                // The viewBox is 44 units tall however many
                                // pixels the box turns out to be, and
                                // `preserve_aspect_ratio: none` stretches it to
                                // fit — so on a filling plot the stroke was
                                // scaled with everything else and came out
                                // several times thicker than the same 1.5 on a
                                // fixed-height plot beside it. This keeps the
                                // width in screen pixels, so every line on the
                                // page is drawn the same weight.
                                vector_effect: "non-scaling-stroke",
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
            }
            if !side_labels.is_empty() {
                div {
                    class: "relative w-44 shrink-0",
                    style: match labels_shift.as_ref() {
                        Some(shift) => format!("margin-top: {shift};"),
                        None => String::new(),
                    },
                    // Fixed slots down the gutter — see `label_tops`. The
                    // half-height shift centres each block on its slot rather
                    // than hanging it below.
                    for (i, l) in side_labels.iter().enumerate() {
                        div {
                            class: "absolute left-0 right-0 -translate-y-1/2",
                            style: "top: {label_tops[i]};",
                            // The swatch is what makes a fixed slot safe.
                            // Position no longer says which curve a reading
                            // names, so the colour does — the same square the
                            // legend row uses, so the two agree on what a
                            // colour means.
                            if let Some(c) = l.color.as_ref() {
                                div { class: "flex items-start gap-2",
                                    span {
                                        // `mt-1` centres an 8px square on the
                                        // 16px first line, and both utilities
                                        // are ones the stylesheet already
                                        // carries — a class Tailwind has not
                                        // seen is simply absent at runtime.
                                        class: "inline-block w-2 h-2 rounded-sm shrink-0 mt-1",
                                        style: "background-color: {c};",
                                    }
                                    div { class: "flex-1 min-w-0", {l.content.clone()} }
                                }
                            } else {
                                {l.content.clone()}
                            }
                        }
                    }
                }
            }
            }

            if show_legend || runtime_change.is_some() {
            div { class: "flex items-center justify-between text-[10px] text-gray-400",
                div { class: "flex items-center gap-3",
                    for s in series.iter().filter(|_| show_legend) {
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
                if show_peak {
                    span { "peak {format_num(peak)}{unit}" }
                }
            }
            }
        }
    }
}

/// A reading drawn beside the plot at the height of `value`.
#[derive(Clone, PartialEq)]
pub struct SideLabel {
    /// The colour of the curve this reading belongs to, drawn as a swatch
    /// beside it. `None` where the plot has one series and there is nothing to
    /// tell apart.
    pub color: Option<String>,
    pub content: Element,
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

/// The peak label's own formatting, so a caller printing the peak elsewhere
/// prints the same number the legend would have.
pub fn format_reading(v: f64) -> String {
    format_num(v)
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
