// src/pages/mod.rs
pub mod config;
pub mod home;
pub mod monitor;
pub mod monitor_jobs;
pub mod monitor_node;
pub mod not_found;

pub use config::Config;
pub use home::Home;
pub use monitor::Monitor;
pub use monitor_jobs::MonitorJobs;
pub use monitor_node::MonitorNode;
pub use not_found::PageNotFound;
