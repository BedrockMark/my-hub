//! Update state - local record of installed versions and ignore lists.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::{info, warn};

use crate::github::release::Release;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateState {
    /// Map of program id -> currently installed version
    pub installed_versions: HashMap<String, InstalledVersion>,

    /// Map of program id -> version string to skip
    pub skip_versions: HashMap<String, String>,

    /// Set of program ids the user has chosen to ignore entirely
    pub ignored_programs: Vec<String>,

    /// Downloads for non-self programs (program id -> downloaded file record)
    #[serde(default)]
    pub downloaded_versions: HashMap<String, DownloadedVersion>,

    /// Last successful update check (ISO 8601 timestamp)
    #[serde(default)]
    pub last_check: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledVersion {
    pub version: String,
    pub installed_at: String,
}

/// Record of a downloaded (but not yet installed) update for a non-self program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedVersion {
    pub version: String,
    pub path: String,
}

/// Live info about a single program's update status - computed, not stored.
#[derive(Debug, Clone)]
pub struct ProgramUpdateInfo {
    pub program_id: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub has_update: bool,
    pub ignored: bool,
    /// Latest release details (notes, assets) - only present when the check succeeded.
    pub release: Option<Release>,
}

impl UpdateState {
    pub fn load(app_data_dir: &Path) -> Result<Self> {
        let path = app_data_dir.join("state.json");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read state.json at {}", path.display()))?;
            match serde_json::from_str::<UpdateState>(&content) {
                Ok(state) => {
                    info!("Loaded state.json");
                    Ok(state)
                }
                Err(e) => {
                    warn!("Failed to parse state.json ({}), starting fresh", e);
                    Ok(UpdateState::default())
                }
            }
        } else {
            info!("No state.json found - starting fresh");
            Ok(UpdateState::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let content = serde_json::to_string_pretty(self).context("Failed to serialize state")?;
        std::fs::write(path, content).with_context(|| format!("Failed to write state to {}", path.display()))?;
        Ok(())
    }

    pub fn is_ignored(&self, program_id: &str) -> bool {
        self.ignored_programs.iter().any(|p| p == program_id)
    }

    pub fn is_version_skipped(&self, program_id: &str, version: &str) -> bool {
        self.skip_versions.get(program_id).map(|v| v == version).unwrap_or(false)
    }
}