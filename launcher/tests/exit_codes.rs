//! The exit codes are a protocol between two languages, and nothing else holds
//! them equal.
//!
//! `be/src/server.ts` decides what to exit with; `launcher/src/main.rs` decides
//! what that means. Neither can see the other's constant, so a change to one is
//! a silent change to the meaning of the other: renumber EXIT_RESTART in the
//! backend and the launcher stops recognising a restart request — it reads an
//! ordinary crash instead, restarts anyway, and the bug looks like a setting
//! that takes two goes to apply.
//!
//! Reading the TypeScript from a test is the same trick `be/test/env-example.test.ts`
//! uses, and for the same reason: a test may parse source, a running program
//! should not.

use std::fs;
use std::path::PathBuf;

fn server_ts() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("launcher/ has a parent")
        .join("be")
        .join("src")
        .join("server.ts");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// `const NAME = 78;` — the only form these are declared in, deliberately kept
/// simple so the test does not need a parser to stay honest.
fn const_value(source: &str, name: &str) -> i32 {
    let needle = format!("const {name} = ");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("be/src/server.ts no longer declares `{name}`"));
    let rest = &source[start + needle.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("`const {name} = ` is not followed by a number"))
}

#[test]
fn the_backend_and_the_launcher_agree_on_restart() {
    assert_eq!(
        const_value(&server_ts(), "EXIT_RESTART"),
        rn::EXIT_RESTART,
        "EXIT_RESTART differs between be/src/server.ts and launcher/src/lib.rs",
    );
}

#[test]
fn the_backend_and_the_launcher_agree_on_fatal() {
    assert_eq!(
        const_value(&server_ts(), "EXIT_FATAL"),
        rn::EXIT_FATAL,
        "EXIT_FATAL differs between be/src/server.ts and launcher/src/lib.rs",
    );
}

#[test]
fn the_two_codes_are_distinct_and_out_of_nodes_own_range() {
    // Node uses 1-13 for its own failures and 128+n for signals. A code inside
    // either range would make a genuine crash indistinguishable from a request.
    assert_ne!(rn::EXIT_RESTART, rn::EXIT_FATAL);
    for code in [rn::EXIT_RESTART, rn::EXIT_FATAL] {
        assert!(code > 13, "{code} collides with Node's own failure codes");
        assert!(code < 128, "{code} collides with the signal range");
    }
}
