//! GitHub API client - fetches release information.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};

use super::release::Release;

/// GitHub API client.
#[derive(Debug, Clone)]
pub struct GitHubClient {
    client: Client,
    pat: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    assets: Vec<ApiAsset>,
}

#[derive(Debug, Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl From<ApiRelease> for Release {
    fn from(api: ApiRelease) -> Self {
        Release {
            tag_name: api.tag_name,
            name: api.name.unwrap_or_default(),
            body: api.body.unwrap_or_default(),
            html_url: api.html_url,
            published_at: api.published_at.unwrap_or_default(),
            assets: api.assets.into_iter().map(|a| super::release::ReleaseAsset {
                name: a.name,
                browser_download_url: a.browser_download_url,
                size: a.size,
            }).collect(),
        }
    }
}

impl GitHubClient {
    pub fn new(pat: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent("my-hub/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self { client, pat })
    }

    /// Fetch the latest release for a GitHub repo.
    pub async fn latest_release(&self, owner: &str, repo: &str) -> Result<Release> {
        let url = format!("https://api.github.com/repos/{}/{}/releases/latest", owner, repo);
        debug!("Fetching latest release from {}", url);

        let mut req = self.client.get(&url).header("Accept", "application/vnd.github+json");
        if let Some(token) = &self.pat {
            req = req.bearer_auth(token);
        }

        let response = req.send().await.context("Failed to send request to GitHub")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // 404 is meaningful: no releases exist yet
            if status.as_u16() == 404 {
                anyhow::bail!("No releases found for {}/{}", owner, repo);
            }
            warn!("GitHub API error {}: {}", status, body);
            anyhow::bail!("GitHub API returned status {} for {}/{}", status, owner, repo);
        }

        let api: ApiRelease = response.json().await.context("Failed to parse GitHub release JSON")?;
        Ok(api.into())
    }
}