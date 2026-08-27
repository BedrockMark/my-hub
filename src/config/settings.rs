//! User settings - persisted in settings.toml.
//!
//! Includes preferences like theme, update-check behavior, PAT, download path.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Whether to check for updates at startup
    #[serde(default = "default_true")]
    pub check_on_startup: bool,

    /// Whether to launch my-hub on Windows startup (registry HKCU\...\Run)
    #[serde(default)]
    pub start_with_windows: bool,

    /// Optional GitHub PAT for higher rate limits
    #[serde(default)]
    pub github_pat: Option<String>,

    /// Directory where downloaded updates are cached
    #[serde(default = "default_download_dir")]
    pub download_dir: String,

    /// Whether downloaded updates should be moved next to the my-hub exe
    /// (into an "installed" subfolder) instead of staying in the download dir.
    #[serde(default)]
    pub move_to_exe_dir: bool,

    /// Theme preference: "Dark" | "Light" | "System"
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Custom colors
    pub custom_colors: CustomColors,
}

fn default_true() -> bool {
    true
}

fn default_download_dir() -> String {
    dirs::cache_dir()
        .map(|p| p.join("my-hub").join("downloads").to_string_lossy().to_string())
        .unwrap_or_else(|| "./downloads".to_string())
}

fn default_theme() -> String {
    "System".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            check_on_startup: true,
            start_with_windows: false,
            github_pat: None,
            download_dir: default_download_dir(),
            move_to_exe_dir: false,
            theme: default_theme(),
            custom_colors: CustomColors::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomColors {
    pub accent: String,
    pub background: String,
    pub text: String,
}

impl Default for CustomColors {
    fn default() -> Self {
        Self {
            accent: "#4a9eff".to_string(),
            background: "#1a1a1a".to_string(),
            text: "#e0e0e0".to_string(),
        }
    }
}

impl Settings {
    pub fn load(app_data_dir: &Path) -> Result<Self> {
        let path = app_data_dir.join("settings.toml");
        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read settings.toml at {}", path.display()))?;
            let settings: Settings = toml::from_str(&content).context("Failed to parse settings.toml")?;
            info!("Loaded settings from {}", path.display());
            Ok(settings)
        } else {
            info!("No settings.toml found - using defaults");
            let settings = Settings::default();
            settings.save(&path)?;
            Ok(settings)
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize settings")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, content).with_context(|| format!("Failed to write settings to {}", path.display()))?;
        Ok(())
    }
}