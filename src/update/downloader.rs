//! Downloader - downloads release assets to disk.

use anyhow::{Context, Result};
use reqwest::Client;
use std::path::PathBuf;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct Downloader {
    client: Client,
    cache_dir: PathBuf,
}

impl Downloader {
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create download cache: {}", cache_dir.display()))?;
        let client = Client::builder()
            .user_agent("my-hub/0.1")
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .context("Failed to build downloader HTTP client")?;
        Ok(Self { client, cache_dir })
    }

    /// Download a URL to the cache dir, return the path to the downloaded file.
    pub async fn download(&self, url: &str, filename: &str) -> Result<PathBuf> {
        let dest = self.cache_dir.join(filename);
        debug!("Downloading {} -> {}", url, dest.display());

        let response = self.client.get(url).send().await.context("Failed to start download")?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("Download failed with status {}", status);
        }

        let bytes = response.bytes().await.context("Failed to read download body")?;
        std::fs::write(&dest, &bytes).with_context(|| format!("Failed to write to {}", dest.display()))?;
        info!("Downloaded {} bytes to {}", bytes.len(), dest.display());
        Ok(dest)
    }
}