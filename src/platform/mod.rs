//! Platform-specific code: app data directory, autostart, etc.

#[cfg(windows)]
pub mod windows;

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Resolve the per-user app data directory for my-hub.
///
/// Uses `%APPDATA%\my-hub` on Windows, with fallback to `~/.config/my-hub`.
pub fn app_data_dir() -> Result<PathBuf> {
    if let Some(base) = dirs::config_dir() {
        return Ok(base.join("my-hub"));
    }
    anyhow::bail!("Could not resolve config directory")
}

/// Enable or disable launching my-hub on Windows login.
pub fn set_autostart(enabled: bool, exe_path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::set_autostart(enabled, exe_path)
    }
    #[cfg(not(windows))]
    {
        let _ = (enabled, exe_path);
        anyhow::bail!("Autostart is only supported on Windows")
    }
}

/// Returns true if the autostart entry currently points at the given exe.
pub fn is_autostart_enabled(exe_path: &std::path::Path) -> Result<bool> {
    #[cfg(windows)]
    {
        windows::is_autostart_enabled(exe_path)
    }
    #[cfg(not(windows))]
    {
        let _ = exe_path;
        Ok(false)
    }
}

/// Locate the running executable's path.
pub fn current_exe_path() -> Result<PathBuf> {
    std::env::current_exe().context("Failed to get current exe path")
}