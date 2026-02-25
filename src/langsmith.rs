use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;

const DEFAULT_API_BASE: &str = "https://api.smith.langchain.com";

#[derive(Debug, Deserialize)]
pub struct Deployment {
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub custom_url: Option<String>,
    #[serde(default)]
    pub resource: Option<DeploymentResource>,
}

#[derive(Debug, Deserialize)]
pub struct DeploymentResource {
    #[serde(default)]
    pub url: Option<String>,
}

impl Deployment {
    pub fn url(&self) -> Option<&str> {
        self.custom_url
            .as_deref()
            .or_else(|| self.resource.as_ref()?.url.as_deref())
    }
}

impl std::fmt::Display for Deployment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = self.url().unwrap_or("(no url)");
        let status = if self.status.is_empty() {
            ""
        } else {
            &self.status
        };
        write!(f, "{:<30} {:<8} {}", self.name, status, url)
    }
}

pub async fn search_deployments(api_key: &str, query: &str) -> Result<Vec<Deployment>> {
    let http = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-api-key"),
        HeaderValue::from_str(api_key)?,
    );

    let url = format!(
        "{}/v1/projects?limit=25&offset=0&name_contains={}",
        DEFAULT_API_BASE,
        urlencoding::encode(query)
    );

    let resp = http.get(&url).headers(headers).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("deployment search failed: {} - {}", status, body);
    }

    let deployments: Vec<Deployment> = resp.json().await?;
    Ok(deployments)
}
