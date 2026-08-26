//! What `be/.env` says, against what the process actually has.
//!
//! The file on its own is not worth a page — most of it already shows up as
//! effective values on Config → Runtime. The gap is what is worth showing: the
//! file is read once, at startup, so editing it and not restarting leaves the
//! two disagreeing with nothing to say so. That is the same failure the Active
//! runtime board exists for, one layer down.
//!
//! **No value of an unrecognised key ever crosses this boundary.** `.env` is a
//! file people put things in, and while `.env.example` says credentials do not
//! belong there, nothing stops one. A key rn does not recognise is a key rn
//! cannot vouch for, so it is reported by name and as set — never by value,
//! the same rule `CredentialRef` follows.

use crate::wire;

wire! {
    /// One environment variable, as the file has it and as the process has it.
    #[serde(rename_all = "camelCase")]
    pub struct EnvEntry {
        pub key: String,
        /// What the file says now. `None` when the file does not set it, and
        /// also when rn does not recognise the key — see `known`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub file_value: Option<String>,
        /// Whether the file mentions it at all. Distinguishes "set to nothing"
        /// from "not set", which read the same in a table of values.
        pub in_file: bool,
        /// What this process has, read from its own environment. `None` when
        /// the process does not have it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub process_value: Option<String>,
        /// Whether rn recognises the key — that is, whether it appears in
        /// `be/.env.example`, which a test holds equal to what `config.ts`
        /// actually reads. An unknown key gets its name shown and nothing else.
        pub known: bool,
        /// The file and the process disagree. Almost always means the file was
        /// edited and the backend has not been restarted since; occasionally
        /// means a real environment variable is overriding the file, which is
        /// the documented precedence and worth being able to see.
        pub drifted: bool,
    }
}

wire! {
    /// GET /api/env.
    #[serde(rename_all = "camelCase")]
    pub struct EnvResponse {
        /// Display path of the file, so a reader knows which one this is.
        pub path: String,
        /// False on an install that never had one — `--env-file-if-exists`
        /// tolerates that, so it is a normal state rather than an error.
        pub exists: bool,
        /// Every key the file sets or the process has, known or not.
        #[serde(default)]
        pub entries: Vec<EnvEntry>,
        /// How many entries disagree, so the page can lead with the answer.
        #[serde(default)]
        pub drifted: u32,
    }
}
