//! Theme system - bridges user settings to Slint color values.

use serde::{Deserialize, Serialize};

use super::settings::Settings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTheme {
    pub background: String,
    pub surface: String,
    pub surface_alt: String,
    pub text: String,
    pub text_muted: String,
    pub accent: String,
    pub border: String,
    pub success: String,
    pub warning: String,
}

impl ResolvedTheme {
    /// Resolve theme based on user settings + system preference.
    pub fn resolve(settings: &Settings, dark_mode: bool) -> Self {
        let colors = &settings.custom_colors;
        if dark_mode {
            Self {
                background: colors.background.clone(),
                surface: lighten(&colors.background, 0.1),
                surface_alt: lighten(&colors.background, 0.16),
                text: colors.text.clone(),
                text_muted: "#888888".to_string(),
                accent: colors.accent.clone(),
                border: "#333333".to_string(),
                success: "#4ade80".to_string(),
                warning: "#fbbf24".to_string(),
            }
        } else {
            // Light mode uses brighter variant of accent
            Self {
                background: "#f5f5f5".to_string(),
                surface: "#ffffff".to_string(),
                surface_alt: "#fafafa".to_string(),
                text: "#1a1a1a".to_string(),
                text_muted: "#666666".to_string(),
                accent: colors.accent.clone(),
                border: "#e0e0e0".to_string(),
                success: "#22c55e".to_string(),
                warning: "#f59e0b".to_string(),
            }
        }
    }
}

/// Lighten a `#rrggbb` hex color by interpolating towards white by `amount` (0.0-1.0).
fn lighten(hex: &str, amount: f32) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return format!("#{}", hex);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f32;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0) as f32;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0) as f32;
    let lerp = |c: f32| (c + (255.0 - c) * amount).round() as u8;
    format!("#{:02x}{:02x}{:02x}", lerp(r), lerp(g), lerp(b))
}