//! Update checker - compares installed versions against latest GitHub releases.

use semver::Version;
use tracing::{debug, info, warn};

use crate::config::manifest::ProgramManifest;
use crate::github::GitHubClient;
use crate::update::state::UpdateState;

use super::state::ProgramUpdateInfo;

pub struct UpdateChecker<'a> {
    manifest: &'a ProgramManifest,
    state: &'a UpdateState,
    client: &'a GitHubClient,
}

impl<'a> UpdateChecker<'a> {
    pub fn new(manifest: &'a ProgramManifest, state: &'a UpdateState, client: &'a GitHubClient) -> Self {
        Self { manifest, state, client }
    }

    /// Check all enabled programs for updates.
    /// Returns one ProgramUpdateInfo per program in the manifest.
    pub async fn check_all(&self) -> Vec<ProgramUpdateInfo> {
        let mut results = Vec::with_capacity(self.manifest.program.len());
        for program in &self.manifest.program {
            let info = self.check_one(program).await;
            results.push(info);
        }
        results
    }

    /// Check a single program for updates.
    pub async fn check_one(&self, program: &crate::config::manifest::ProgramEntry) -> ProgramUpdateInfo {
        let current_version = self
            .state
            .installed_versions
            .get(&program.id)
            .map(|v| v.version.clone());

        if !program.enabled {
            debug!("Program {} is disabled, skipping", program.id);
            return ProgramUpdateInfo {
                program_id: program.id.clone(),
                current_version,
                latest_version: None,
                has_update: false,
                ignored: self.state.is_ignored(&program.id),
                release: None,
            };
        }

        match self.client.latest_release(&program.owner, &program.repo).await {
            Ok(release) => {
                let latest_version = parse_version(&release.tag_name);
                let has_update = match (&current_version, &latest_version) {
                    (Some(cur), Some(lat)) => match (Version::parse(cur), Version::parse(lat)) {
                        (Ok(c), Ok(l)) => l > c,
                        _ => cur != lat,
                    },
                    (None, Some(_)) => true, // not installed yet
                    _ => false,
                };

                ProgramUpdateInfo {
                    program_id: program.id.clone(),
                    current_version,
                    latest_version,
                    has_update,
                    ignored: self.state.is_ignored(&program.id),
                    release: Some(release),
                }
            }
            Err(e) => {
                warn!("Failed to check {}: {}", program.id, e);
                ProgramUpdateInfo {
                    program_id: program.id.clone(),
                    current_version,
                    latest_version: None,
                    has_update: false,
                    ignored: self.state.is_ignored(&program.id),
                    release: None,
                }
            }
        }
    }
}

/// Parse a version string, stripping leading 'v' if present.
fn parse_version(tag: &str) -> Option<String> {
    let trimmed = tag.trim_start_matches('v');
    // Try strict semver first; if it fails, try cleaning common suffixes
    if Version::parse(trimmed).is_ok() {
        Some(trimmed.to_string())
    } else {
        // Strip pre-release/build metadata
        let core = trimmed.split('-').next().unwrap_or(trimmed);
        if Version::parse(core).is_ok() {
            Some(core.to_string())
        } else {
            info!("Could not parse version from tag '{}'", tag);
            None
        }
    }
}