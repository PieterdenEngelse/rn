// src/pages/mod.rs
pub mod config;
pub mod home;
pub mod monitor;
pub mod not_found;

pub use config::Config;
pub use home::Home;
pub use monitor::Monitor;
pub use not_found::PageNotFound;
