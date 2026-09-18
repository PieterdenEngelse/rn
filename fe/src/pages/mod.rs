// src/pages/mod.rs

/// The jobs whose cards live on the mail pages rather than the jobs pages.
///
/// Here rather than in one of the four pages that consult it, because with a
/// copy per page they could overlap — a job on both — or, worse, disagree in
/// the other direction and leave a job reachable from neither.
///
/// A written list rather than something derived. The obvious derivation is
/// "wants the gmailAppPassword credential", and it is a coincidence: a job
/// could want that credential without belonging on a mail page, and a mail job
/// could arrive without wanting it. Nothing on the wire says which page owns a
/// job, so this says it.
///
/// A third mail job not named here appears on the jobs pages instead, which is
/// the visible failure rather than the silent one.
pub const MAIL_JOB_IDS: [&str; 2] = ["read-mail", "send-mail"];

pub mod config;
pub mod config_connection;
pub mod config_mail;
pub mod config_jobs;
pub mod config_watching;
pub mod home;
pub mod monitor_connection;
pub mod links;
pub mod monitor_jobs;
pub mod monitor_mail;
pub mod monitor_runtime;
pub mod not_found;

pub use config::Config;
pub use config_connection::ConfigConnection;
pub use config_mail::ConfigMail;
pub use config_jobs::ConfigJobs;
pub use config_watching::ConfigWatching;
pub use home::Home;
pub use monitor_connection::MonitorConnection;
pub use links::{MonitorLinks, MonitorLinksSend};
pub use monitor_jobs::MonitorJobs;
pub use monitor_mail::MonitorMail;
pub use monitor_runtime::MonitorRuntime;
pub use not_found::PageNotFound;
