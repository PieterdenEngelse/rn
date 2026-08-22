//! Shared components — the building blocks of the app.

pub mod header;
pub mod info;
pub mod panel;
pub mod process_panel;
pub use process_panel::ProcessPanel;
pub mod restart_banner;
pub mod sparkline;
pub use sparkline::{Series, Sparkline};
pub mod status_light;
pub mod sub_nav;
pub use sub_nav::SubNav;
pub use status_light::StatusLight;
pub use restart_banner::RestartBanner;
pub mod param;
pub mod runtime_board;
pub use runtime_board::RuntimeBoard;
pub use panel::Panel;
pub use info::{GlossaryEntry, InfoButton};
pub use header::Header;
