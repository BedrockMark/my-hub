//! Application state - top-level coordinator for all subsystems.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

use crate::config::manifest::ProgramManifest;
use crate::config::settings::Settings;
use crate::update::state::{ProgramUpdateInfo, UpdateState};

pub struct AppState {
    /// Resolved manifest (default + user overrides merged)
    pub manifest: ProgramManifest,
    /// User settings
    pub settings: Settings,
    /// Local update state (installed versions, ignore lists)
    pub update_state: UpdateState,
    /// Path to the app data directory
    pub app_data_dir: PathBuf,
    /// In-memory cache of the last update check (program id -> result).
    /// Not persisted; used by the update dialog and download flow.
    pub check_results: HashMap<String, ProgramUpdateInfo>,
}

impl AppState {
    pub fn initialize() -> Result<Self> {
        // Resolve app data directory
        let app_data_dir = crate::platform::app_data_dir().context("Failed to resolve app data directory")?;
        std::fs::create_dir_all(&app_data_dir)
            .with_context(|| format!("Failed to create app data directory: {}", app_data_dir.display()))?;
        info!("App data dir: {}", app_data_dir.display());

        // Load manifest (default + user override)
        let manifest = ProgramManifest::load(&app_data_dir).context("Failed to load program manifest")?;

        // Load settings
        let settings = Settings::load(&app_data_dir).context("Failed to load settings")?;

        // Load update state
        let update_state = UpdateState::load(&app_data_dir).context("Failed to load update state")?;

        Ok(Self {
            manifest,
            settings,
            update_state,
            app_data_dir,
            check_results: HashMap::new(),
        })
    }

    /// Path to the settings.toml
    pub fn settings_path(&self) -> PathBuf {
        self.app_data_dir.join("settings.toml")
    }

    /// Path to the state.json
    pub fn state_path(&self) -> PathBuf {
        self.app_data_dir.join("state.json")
    }

    /// Save all state (settings + update state + manifest overrides)
    pub fn save(&self) -> Result<()> {
        self.settings.save(&self.settings_path())?;
        self.update_state.save(&self.state_path())?;
        Ok(())
    }
}