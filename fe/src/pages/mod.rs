// src/pages/mod.rs
pub mod config;
pub mod config_connection;
pub mod config_jobs;
pub mod home;
pub mod monitor_connection;
pub mod links;
pub mod monitor_jobs;
pub mod monitor_runtime;
pub mod not_found;

pub use config::Config;
pub use config_connection::ConfigConnection;
pub use config_jobs::ConfigJobs;
pub use home::Home;
pub use monitor_connection::MonitorConnection;
pub use links::MonitorLinks;
pub use monitor_jobs::MonitorJobs;
pub use monitor_runtime::MonitorRuntime;
pub use not_found::PageNotFound;
