//! GitHub integration - fetch releases and asset metadata.

pub mod client;
pub mod release;

pub use client::GitHubClient;