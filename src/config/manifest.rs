//! Program manifest - describes the my-family suite of programs.
//!
//! Loaded from `assets/default-manifest.toml` (bundled) merged with optional
//! `manifest.toml` in the app data directory (user overrides).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

const DEFAULT_MANIFEST: &str = include_str!("../../assets/default-manifest.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramEntry {
    /// Unique slug used as key in state.json
    pub id: String,
    /// Display name shown in the UI
    pub name: String,
    /// GitHub owner (user or org)
    pub owner: String,
    /// GitHub repository name
    pub repo: String,
    /// Executable name within the release asset (e.g. "my-clipboard.exe")
    pub executable: String,
    /// Optional short description
    #[serde(default)]
    pub description: String,
    /// Whether to check for updates
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgramManifest {
    pub program: Vec<ProgramEntry>,
}

impl ProgramManifest {
    /// Load the manifest: start from bundled default, then merge user overrides.
    pub fn load(app_data_dir: &Path) -> Result<Self> {
        let mut manifest: ProgramManifest =
            toml::from_str(DEFAULT_MANIFEST).context("Failed to parse bundled default manifest")?;

        let user_path = app_data_dir.join("manifest.toml");
        if user_path.exists() {
            let user_content = std::fs::read_to_string(&user_path)
                .with_context(|| format!("Failed to read user manifest at {}", user_path.display()))?;
            let user_manifest: ProgramManifest =
                toml::from_str(&user_content).context("Failed to parse user manifest")?;
            info!("Loaded user manifest override: {} programs", user_manifest.program.len());

            // Merge: user entries override defaults by id
            for user_entry in user_manifest.program {
                if let Some(existing) = manifest.program.iter_mut().find(|p| p.id == user_entry.id) {
                    *existing = user_entry;
                } else {
                    manifest.program.push(user_entry);
                }
            }
        } else {
            warn!(
                "No user manifest at {} - using bundled default",
                user_path.display()
            );
        }

        Ok(manifest)
    }

    /// Find a program by id.
    pub fn find(&self, id: &str) -> Option<&ProgramEntry> {
        self.program.iter().find(|p| p.id == id)
    }
}