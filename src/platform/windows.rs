//! Windows-specific platform code.

use anyhow::{Context, Result};
use std::path::Path;
use tracing::info;
use winreg::enums::*;
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "my-hub";

pub fn set_autostart(enabled: bool, exe_path: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = hkcu
        .open_subkey_with_flags(RUN_KEY_PATH, KEY_SET_VALUE | KEY_QUERY_VALUE)
        .context("Failed to open HKCU Run registry key")?;

    if enabled {
        let command = format!("\"{}\"", exe_path.display());
        run_key
            .set_value(VALUE_NAME, &command)
            .context("Failed to set autostart registry value")?;
        info!("Autostart enabled: {}", command);
    } else {
        // Best-effort delete - it's fine if the value doesn't exist
        match run_key.delete_value(VALUE_NAME) {
            Ok(_) => info!("Autostart disabled"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!("Autostart was not set - nothing to remove");
            }
            Err(e) => {
                return Err(anyhow::Error::from(e).context("Failed to remove autostart registry value"));
            }
        }
    }
    Ok(())
}

pub fn is_autostart_enabled(exe_path: &Path) -> Result<bool> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run_key = match hkcu.open_subkey(RUN_KEY_PATH) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(anyhow::Error::from(e).context("Failed to open HKCU Run registry key")),
    };

    let value: Result<String, _> = run_key.get_value(VALUE_NAME);
    match value {
        Ok(s) => Ok(s.contains(&exe_path.display().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(anyhow::Error::from(e).context("Failed to query autostart registry value")),
    }
}