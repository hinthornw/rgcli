use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;

const DEFAULT_HOST_URL: &str = "https://api.smith.langchain.com";
const DEFAULT_PROJECTS_URL: &str = "https://api.host.langchain.com";

pub struct HostClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
pub struct PushTokenResponse {
    pub token: String,
    pub registry_url: String,
    #[allow(dead_code)]
    pub expires_at: String,
}

impl HostClient {
    pub fn new(api_key: &str) -> Result<Self> {
        let base_url = std::env::var("LANGSMITH_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_HOST_URL.to_string())
            .trim_end_matches('/')
            .to_string();

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert(
            "X-Api-Key",
            HeaderValue::from_str(api_key).context("invalid API key")?,
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("ailsd")
            .build()?;

        Ok(Self { http, base_url })
    }

    /// List all deployments via /v1/projects. Paginates with limit/offset.
    pub async fn list_deployments(&self) -> Result<Vec<serde_json::Value>> {
        let mut all = Vec::new();
        let limit = 100;
        let mut offset = 0;
        loop {
            let projects_base = std::env::var("LANGSMITH_HOST_URL")
                .unwrap_or_else(|_| DEFAULT_PROJECTS_URL.to_string());
            let url = format!(
                "{}/v1/projects?limit={}&offset={}",
                projects_base.trim_end_matches('/'),
                limit,
                offset
            );
            let resp = self.http.get(&url).send().await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("GET /v1/projects failed: {status} - {body}");
            }
            let batch: Vec<serde_json::Value> = resp.json().await?;
            let count = batch.len();
            all.extend(batch);
            if count < limit {
                break;
            }
            offset += limit;
        }
        Ok(all)
    }

    /// Find a deployment by name. Returns (id, deployment_json) if found.
    pub async fn find_deployment_by_name(
        &self,
        name: &str,
    ) -> Result<Option<(String, serde_json::Value)>> {
        let deployments = self.list_deployments().await?;
        for d in deployments {
            if d.get("name").and_then(|n| n.as_str()) == Some(name) {
                let id = d
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string();
                return Ok(Some((id, d)));
            }
        }
        Ok(None)
    }

    /// Create a new internal_docker deployment.
    pub async fn create_deployment(
        &self,
        name: &str,
        deployment_type: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/v2/deployments", self.base_url);
        let body = serde_json::json!({
            "name": name,
            "source": "internal_docker",
            "source_config": {
                "deployment_type": deployment_type,
            },
            "source_revision_config": {},
            "secrets": [],
        });

        let resp = self.http.post(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /v2/deployments failed: {status} - {body}");
        }
        Ok(resp.json().await?)
    }

    /// Get a push token for the deployment's Artifact Registry.
    pub async fn get_push_token(&self, deployment_id: &str) -> Result<PushTokenResponse> {
        let url = format!(
            "{}/v2/deployments/{}/push-token",
            self.base_url, deployment_id
        );
        let resp = self.http.post(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST push-token failed: {status} - {body}");
        }
        Ok(resp.json().await?)
    }

    /// Patch deployment with image_uri to trigger a new revision.
    pub async fn patch_deployment(
        &self,
        deployment_id: &str,
        image_uri: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/v2/deployments/{}", self.base_url, deployment_id);
        let body = serde_json::json!({
            "source_revision_config": {
                "image_uri": image_uri,
            },
        });

        let resp = self.http.patch(&url).json(&body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("PATCH deployment failed: {status} - {body}");
        }
        Ok(resp.json().await?)
    }
}
