//! The runtime-parameter and configuration surface: everything Config →
//! Runtime reads and writes, plus the process status the header light reads.
//!
//! These were the last shapes in rn defined twice — `fe/src/api/wire.rs` had
//! eleven hand-written structs, `be/src/runtime-params.ts` and
//! `be/src/settings.ts` described the same fields independently, and the two
//! agreed only because someone was careful.
//!
//! The five closed sets — `ParamKind`, `ParamType`, `AppliesAt`, `Category`
//! and `JsRuntime` — are enums rather than strings on purpose. `be` typed them
//! as string-literal unions before the move, so `category: "memry"` was a
//! compile error there; expressing them as `String` here would have bought
//! `fe`'s convenience by taking that guard away from a registry of a thousand
//! lines. `fe` matches on the variants instead.

use crate::wire;
use crate::RunningJob;
use serde_json::Value;

wire! {
    /// How a parameter reaches the runtime, which decides who delivers it.
    ///
    /// `app` is rn's own setting rather than the runtime's: no flag, no
    /// environment variable, nothing for the launcher to pass on. The launcher
    /// must skip these explicitly — its `resolve()` falls through to
    /// NODE_OPTIONS for any kind it does not recognise, so an unhandled one
    /// would emit `NODE_OPTIONS="--schedulerTickMs=30000"` and stop Node
    /// booting before the first line of code runs.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum ParamKind {
        Env,
        NodeOption,
        Launcher,
        RuntimeFlag,
        App,
    }
}

wire! {
    /// What kind of value a parameter takes, which decides the control drawn
    /// for it and the validation applied to it.
    ///
    /// `enum-open` is a closed list of suggestions over a field that still
    /// accepts anything typed into it — the browser draws the list its own way
    /// and a browser that ignores the decoration still leaves a usable field.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum ParamType {
        Int,
        #[serde(rename = "string")]
        Str,
        Bool,
        Enum,
        EnumOpen,
    }
}

wire! {
    /// When a change takes hold.
    ///
    /// Almost everything is read once at process start by libuv, ICU or the
    /// TLS stack, so changing it means relaunching. That is not a limitation to
    /// hide: the UI says which of the two a control is, next to the control.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum AppliesAt {
        Restart,
        Runtime,
    }
}

wire! {
    /// The board a parameter is filed under on Config → Runtime.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum Category {
        Memory,
        Concurrency,
        Time,
        Network,
        /// The mail account rn sends and reads with, and the two servers it
        /// uses. Its own board rather than a corner of Network, because these
        /// are rn's own settings and Network is the *runtime's* — a page that
        /// filed them together showed two boards both titled "Network", one
        /// per tile, which is unreadable however correct each half is.
        Mail,
        /// Click tracking on mail rn sends — where links point, and how long
        /// identity is kept. Its own board rather than a corner of Network,
        /// because the settings here are read together with Monitor → Links
        /// and one of them is the only setting in rn whose mistake cannot be
        /// corrected afterwards.
        Links,
        Diagnostics,
        Output,
        Runtime,
        Security,
    }
}

wire! {
    /// A JavaScript runtime rn can be asked to run under.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum JsRuntime {
        Node,
        Bun,
        Deno,
    }
}

wire! {
    /// Set when the flag belongs to V8 rather than to the runtime around it.
    ///
    /// It decides delivery, not presentation: Node accepts V8 flags in
    /// NODE_OPTIONS, but Deno runs V8 while ignoring NODE_OPTIONS, so its V8
    /// flags have to be folded into `--v8-flags` instead. A Node flag that ends
    /// up there does not boot.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum Engine {
        V8,
    }
}

wire! {
    /// The three lines of an info panel, for a runtime parameter.
    ///
    /// The same shape a job's `JobInfo` carries, and rendered by the same
    /// `InfoButton` — see "Info buttons" in CLAUDE.md. Making the text data
    /// rather than markup is what stops a parameter being added without one.
    pub struct ParamInfo {
        /// What it does, mechanically.
        pub what: String,
        /// Why a user would touch it.
        pub why: String,
        /// What they will see if it is wrong.
        #[serde(rename = "ifWrong")]
        pub if_wrong: String,
    }
}

wire! {
    /// One allowed value of an `enum` parameter.
    pub struct ParamOption {
        pub value: String,
        pub label: String,
        /// Present when the option explains itself; the UI prefers it over the
        /// parameter's own panel for the current selection. Three runtimes
        /// flattened into a single panel is three explanations nobody reads.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub info: Option<ParamInfo>,
    }
}

wire! {
    /// One user-editable runtime parameter, as the registry describes it.
    ///
    /// `be/src/runtime-params.ts` holds the entries; this is their shape. Two
    /// files are generated from those entries — `docs/node-parameters.md` and
    /// `be/runtime-params.json`, the latter read by the Rust launcher — so a
    /// field added here has three readers before it has a control.
    pub struct RuntimeParam {
        /// Stable key used in settings.json. Never rename — it is persisted.
        pub id: String,
        /// The environment variable name, or the flag.
        pub flag: String,
        pub kind: ParamKind,
        #[serde(rename = "type")]
        pub value_type: ParamType,
        /// `null` means "unset — inherit the system default".
        pub default: Value,
        /// Key in the `effective` payload whose live value stands in for the
        /// default. Set where the default is whatever the OS reports, so the
        /// field can name it instead of just saying the value is unset.
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "defaultFrom")]
        pub default_from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub unit: Option<String>,
        // `i64` alone emits `bigint`, which the registry's plain `min: 1024`
        // is not assignable to. TypeScript has one number type and `be` always
        // treated these as that, so the annotation says so.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
        pub min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "typescript", ts(type = "number | null"))]
        pub max: Option<i64>,
        /// What the "no value chosen" entry of a nullable enum should read.
        ///
        /// A dropdown cannot show a placeholder the way a text field can, so an
        /// unset enum needs a real option to sit on, and that option needs
        /// wording only the registry has: "unset" is true but says nothing,
        /// while `nodeVersion`'s "Bundled runtime — no version pinned" says
        /// what happens. It lives here for the reason every other string on a
        /// parameter does — the page must not be able to invent copy about a
        /// setting. Absent falls back to "unset", which is right for a value
        /// whose default needs no explanation.
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "unsetLabel")]
        pub unset_label: Option<String>,
        /// Required when `value_type` is `Enum`; suggestions when `EnumOpen`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub options: Option<Vec<ParamOption>>,
        /// Runtimes this parameter does anything on. `None` means all of them.
        /// Bun and Deno tolerate a NODE_OPTIONS they do not implement rather
        /// than refusing to start, so a Node-only flag under them is silently
        /// ignored — the UI has to say so, because nothing else will.
        #[serde(default, skip_serializing_if = "Option::is_none", rename = "appliesTo")]
        pub applies_to: Option<Vec<JsRuntime>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub engine: Option<Engine>,
        #[serde(rename = "appliesAt")]
        pub applies_at: AppliesAt,
        pub category: Category,
        pub label: String,
        pub info: ParamInfo,
    }
}

wire! {
    /// A flag the registry deliberately does not offer, and why.
    ///
    /// Listed rather than omitted: a control that is missing looks like an
    /// oversight, and the reason it is missing is usually the interesting part.
    pub struct Withheld {
        pub flag: String,
        pub reason: String,
    }
}

wire! {
    /// A saved setting that the running process does not have.
    ///
    /// Computed by comparing what the settings resolve to against what the
    /// process actually got, rather than by remembering the last save — so the
    /// banner survives a reload, and clears itself once a restart has genuinely
    /// applied the change.
    pub struct PendingChange {
        pub id: String,
        pub label: String,
        /// What the settings ask for.
        pub want: String,
        /// What the running process actually has.
        pub have: String,
    }
}

wire! {
    /// `GET /api/params`: the registry, the live values, and the saved settings.
    pub struct ParamsResponse {
        pub params: Vec<RuntimeParam>,
        pub withheld: Vec<Withheld>,
        /// What the process is actually running with, keyed by parameter id.
        pub effective: Value,
        /// What is saved in settings.json, keyed by parameter id.
        pub settings: Value,
        /// True when a launcher supervises the backend and can restart it.
        #[serde(default)]
        pub supervised: bool,
        #[serde(default)]
        pub pending: Vec<PendingChange>,
    }
}

wire! {
    /// One rejected setting, named so the UI can mark the control rather than
    /// the form.
    pub struct SaveError {
        pub id: String,
        pub message: String,
    }
}

wire! {
    /// `PUT /api/settings`: what was taken, what needs a restart, what failed.
    pub struct SaveResponse {
        pub ok: bool,
        /// Settings that took effect immediately, so the UI's "(immediate)"
        /// label is true rather than aspirational.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub applied: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "restartRequired")]
        pub restart_required: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub errors: Vec<SaveError>,
    }
}

wire! {
    /// `POST /api/restart`: whether it happened, was queued, or was refused.
    ///
    /// Refused is a real answer here — an unsupervised process that exits does
    /// not come back, so it says so instead of leaving a dead server and no
    /// explanation.
    pub struct RestartOutcome {
        #[serde(default)]
        pub ok: bool,
        /// True when the restart is queued behind running work rather than
        /// done. Aborting a long automation to apply a setting is the failure
        /// this guards against.
        #[serde(default)]
        pub scheduled: bool,
        #[serde(default)]
        pub message: String,
        /// Set when the request was refused.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub error: Option<String>,
        /// Jobs still running, when a restart was queued behind them.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub running: Vec<RunningJob>,
        /// Jobs a `when=now` restart interrupted.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub aborted: Vec<RunningJob>,
    }
}

wire! {
    /// `POST /api/stop`: the same three answers as a restart, minus the queue.
    pub struct StopOutcome {
        #[serde(default)]
        pub ok: bool,
        #[serde(default)]
        pub message: String,
        /// Set when the request was refused — work in progress, without force.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub error: Option<String>,
        /// Jobs whose presence refused the request.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub running: Vec<RunningJob>,
        /// Jobs the stop interrupted.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub aborted: Vec<RunningJob>,
    }
}

wire! {
    /// `GET /api/status`: what is running, under what, since when.
    ///
    /// Carries the two counts the header light needs so it does not cost a
    /// second request per poll.
    pub struct StatusResponse {
        pub supervised: bool,
        pub pid: u32,
        /// A string rather than a number: it arrives from the environment, and
        /// `null` when nothing is supervising.
        #[serde(rename = "launcherPid")]
        pub launcher_pid: Option<String>,
        #[serde(rename = "uptimeMs")]
        pub uptime_ms: f64,
        pub node: String,
        #[serde(rename = "execPath")]
        pub exec_path: String,
        #[serde(rename = "settingsPath")]
        pub settings_path: String,
        pub url: String,
        pub jobs: u32,
        #[serde(default, rename = "restartPending")]
        pub restart_pending: bool,
        /// Saved settings not yet in effect — drives the amber header light.
        #[serde(default, rename = "pendingCount")]
        pub pending_count: u32,
        /// Jobs whose most recent run failed — drives the red header light.
        /// Most-recent, not ever-failed, so a successful re-run clears it.
        #[serde(default, rename = "failedJobs")]
        pub failed_jobs: u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire spellings, pinned.
    ///
    /// These six enums replaced string-literal unions that `be` had typed by
    /// hand, so the words below are the entire agreement between the two ends:
    /// rename a variant without a `#[serde(rename)]` and `fe` stops parsing
    /// `/api/params` altogether — not one field going `undefined`, the whole
    /// page failing to load. Cheap to pin, expensive to notice otherwise.
    #[test]
    fn the_closed_sets_spell_themselves_the_way_the_registry_does() {
        fn one<T: serde::Serialize>(v: T) -> String {
            serde_json::to_string(&v).expect("a fieldless enum serialises")
        }
        assert_eq!(one(&ParamKind::NodeOption), "\"node-option\"");
        assert_eq!(one(&ParamKind::RuntimeFlag), "\"runtime-flag\"");
        assert_eq!(one(&ParamKind::Env), "\"env\"");
        assert_eq!(one(&ParamKind::Launcher), "\"launcher\"");
        assert_eq!(one(&ParamKind::App), "\"app\"");

        assert_eq!(one(&ParamType::Int), "\"int\"");
        assert_eq!(one(&ParamType::Str), "\"string\"");
        assert_eq!(one(&ParamType::Bool), "\"bool\"");
        assert_eq!(one(&ParamType::Enum), "\"enum\"");
        assert_eq!(one(&ParamType::EnumOpen), "\"enum-open\"");

        assert_eq!(one(&AppliesAt::Restart), "\"restart\"");
        assert_eq!(one(&AppliesAt::Runtime), "\"runtime\"");

        assert_eq!(one(&Category::Memory), "\"memory\"");
        assert_eq!(one(&Category::Concurrency), "\"concurrency\"");
        assert_eq!(one(&Category::Time), "\"time\"");
        assert_eq!(one(&Category::Network), "\"network\"");
        assert_eq!(one(&Category::Mail), "\"mail\"");
        assert_eq!(one(&Category::Links), "\"links\"");
        assert_eq!(one(&Category::Diagnostics), "\"diagnostics\"");
        assert_eq!(one(&Category::Output), "\"output\"");
        assert_eq!(one(&Category::Runtime), "\"runtime\"");
        assert_eq!(one(&Category::Security), "\"security\"");

        assert_eq!(one(&JsRuntime::Node), "\"node\"");
        assert_eq!(one(&JsRuntime::Bun), "\"bun\"");
        assert_eq!(one(&JsRuntime::Deno), "\"deno\"");

        assert_eq!(one(&Engine::V8), "\"v8\"");
    }

    /// A parameter as `be` actually sends one, including the absent keys.
    ///
    /// The optional fields are the ones that bite: `min`, `max`, `unit`,
    /// `defaultFrom`, `options`, `appliesTo` and `engine` are all omitted from
    /// most registry entries, and a missing `#[serde(default)]` on any of them
    /// would fail the whole response rather than that one field.
    #[test]
    fn a_parameter_with_every_optional_key_absent_still_parses() {
        let json = r#"{
            "id": "maxOldSpaceSize",
            "flag": "--max-old-space-size",
            "kind": "node-option",
            "type": "int",
            "default": null,
            "appliesAt": "restart",
            "category": "memory",
            "label": "Heap limit",
            "info": { "what": "w", "why": "y", "ifWrong": "i" }
        }"#;
        let p: RuntimeParam = serde_json::from_str(json).expect("parses without the optional keys");
        assert_eq!(p.kind, ParamKind::NodeOption);
        assert_eq!(p.value_type, ParamType::Int);
        assert_eq!(p.category, Category::Memory);
        assert_eq!(p.applies_at, AppliesAt::Restart);
        assert!(p.default.is_null());
        assert_eq!(p.min, None);
        assert_eq!(p.options, None);
        assert_eq!(p.applies_to, None);
        assert_eq!(p.engine, None);
    }

    /// The refusal shapes, which carry `error` and omit the collections.
    #[test]
    fn a_refused_restart_parses_without_its_job_lists() {
        let out: RestartOutcome = serde_json::from_str(
            r#"{"ok":false,"scheduled":false,"error":"not supervised","message":"no launcher"}"#,
        )
        .expect("parses");
        assert!(!out.ok);
        assert_eq!(out.error.as_deref(), Some("not supervised"));
        assert!(out.running.is_empty() && out.aborted.is_empty());
    }
}
