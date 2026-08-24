//! my-hub - A central hub for managing the my-family suite of applications.
//!
//! This binary is the entry point. It wires together the config layer,
//! GitHub update checker, and Slint GUI.

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod app_state;
mod config;
mod github;
mod platform;
mod ui_bridge;
mod update;

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::app_state::AppState;

fn main() -> Result<()> {
    // Initialize logging
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();

    info!("my-hub starting up");

    // Build the application state (config + manifest + state file loaded here)
    let state = AppState::initialize().context("Failed to initialize app state")?;

    // Build and run the Slint UI
    ui_bridge::run(state).context("UI failed to run")?;

    info!("my-hub exiting");
    Ok(())
}