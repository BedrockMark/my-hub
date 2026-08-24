//! Update subsystem - check for updates, download new versions, manage state.

pub mod checker;
pub mod downloader;
pub mod state;

pub use checker::UpdateChecker;
pub use downloader::Downloader;
pub use state::ProgramUpdateInfo;