//! Browser clipboard access.
//!
//! Not part of the backend boundary — it lived in `api.rs` only because that
//! was where the `web-sys` import already existed.

/// Copy text to the clipboard. Fire-and-forget: the promise runs even though
/// we do not await it, and there is nothing useful to do if it is rejected.
pub fn copy_to_clipboard(text: &str) {
    if let Some(win) = web_sys::window() {
        let _ = win.navigator().clipboard().write_text(text);
    }
}
