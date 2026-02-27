use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;

use super::error::{SandboxError, parse_http_error};
use super::models::*;
use super::runtime::Sandbox;

const DEFAULT_ENDPOINT: &str = "https://api.smith.langchain.com";

/// Client for sandbox control plane operations (CRUD).
#[derive(Debug, Clone)]
pub struct SandboxClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl SandboxClient {
    /// Create a new sandbox client. Reads LANGSMITH_ENDPOINT and LANGSMITH_API_KEY
    /// from environment if not provided.
    pub fn new(api_key: &str) -> Result<Self, SandboxError> {
        let endpoint =
            std::env::var("LANGSMITH_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        Self::new_with_endpoint(api_key, &endpoint)
    }

    /// Create a new sandbox client with an explicit LangSmith endpoint.
    pub fn new_with_endpoint(api_key: &str, endpoint: &str) -> Result<Self, SandboxError> {
        let base_url = format!("{}/v2/sandboxes", endpoint.trim_end_matches('/'));

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        headers.insert(
            "X-Api-Key",
            HeaderValue::from_str(api_key).map_err(|e| SandboxError::Auth(e.to_string()))?,
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("ailsd-sandbox")
            .build()?;

        Ok(Self {
            http,
            base_url,
            api_key: api_key.to_string(),
        })
    }

    fn dataplane_http_for(api_key: &str) -> reqwest::Client {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(api_key) {
            headers.insert("X-Api-Key", val);
        }
        reqwest::Client::builder()
            .default_headers(headers)
            .user_agent("ailsd-sandbox")
            .build()
            .unwrap_or_default()
    }

    /// Build an HTTP client for dataplane operations (includes API key auth).
    fn dataplane_http(&self) -> reqwest::Client {
        Self::dataplane_http_for(&self.api_key)
    }

    /// Construct a sandbox handle for a known dataplane URL and auth token.
    ///
    /// This is useful for server-issued relay sessions where the dataplane URL
    /// and token come from a separate control plane.
    pub fn sandbox_from_dataplane(
        &self,
        name: &str,
        dataplane_url: &str,
        auth_token: &str,
    ) -> Sandbox {
        let info = SandboxInfo {
            name: name.to_string(),
            template_name: "relay".to_string(),
            dataplane_url: Some(dataplane_url.trim_end_matches('/').to_string()),
            id: None,
            created_at: None,
            updated_at: None,
        };
        Sandbox::new(
            info,
            Self::dataplane_http_for(auth_token),
            auth_token.to_string(),
        )
    }

    // ── Helpers ──

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, SandboxError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_http_error(status.as_u16(), &body));
        }
        Ok(resp.json().await?)
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, SandboxError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.post(&url).json(body).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_http_error(status.as_u16(), &body));
        }
        Ok(resp.json().await?)
    }

    async fn delete(&self, path: &str) -> Result<(), SandboxError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.delete(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(parse_http_error(status.as_u16(), &body));
        }
        Ok(())
    }

    // ── Sandboxes ──

    /// Create a new sandbox from a template.
    pub async fn create_sandbox(
        &self,
        template_name: &str,
        name: Option<&str>,
    ) -> Result<Sandbox, SandboxError> {
        let mut body = serde_json::json!({ "template_name": template_name });
        if let Some(n) = name {
            body["name"] = serde_json::Value::String(n.to_string());
        }
        let info: SandboxInfo = self.post_json("/boxes", &body).await?;
        Ok(Sandbox::new(
            info,
            self.dataplane_http(),
            self.api_key.clone(),
        ))
    }

    /// Get an existing sandbox by name.
    pub async fn get_sandbox(&self, name: &str) -> Result<Sandbox, SandboxError> {
        let info: SandboxInfo = self
            .get_json(&format!("/boxes/{}", urlencoding::encode(name)))
            .await?;
        Ok(Sandbox::new(
            info,
            self.dataplane_http(),
            self.api_key.clone(),
        ))
    }

    /// List all sandboxes.
    pub async fn list_sandboxes(&self) -> Result<Vec<SandboxInfo>, SandboxError> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BoxesResponse {
            Direct(Vec<SandboxInfo>),
            Wrapped { boxes: Vec<SandboxInfo> },
            WrappedSandboxes { sandboxes: Vec<SandboxInfo> },
        }

        let parsed: BoxesResponse = self.get_json("/boxes").await?;
        Ok(match parsed {
            BoxesResponse::Direct(items) => items,
            BoxesResponse::Wrapped { boxes } => boxes,
            BoxesResponse::WrappedSandboxes { sandboxes } => sandboxes,
        })
    }

    /// Delete a sandbox by name.
    pub async fn delete_sandbox(&self, name: &str) -> Result<(), SandboxError> {
        self.delete(&format!("/boxes/{}", urlencoding::encode(name)))
            .await
    }

    // ── Templates ──

    /// Create a new sandbox template.
    pub async fn create_template(
        &self,
        spec: &CreateTemplate,
    ) -> Result<SandboxTemplate, SandboxError> {
        let body = serde_json::to_value(spec).map_err(|e| SandboxError::Validation {
            message: e.to_string(),
            details: vec![],
        })?;
        self.post_json("/templates", &body).await
    }

    /// Get a template by name.
    pub async fn get_template(&self, name: &str) -> Result<SandboxTemplate, SandboxError> {
        self.get_json(&format!("/templates/{}", urlencoding::encode(name)))
            .await
    }

    /// List all templates.
    pub async fn list_templates(&self) -> Result<Vec<SandboxTemplate>, SandboxError> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum TemplatesResponse {
            Direct(Vec<SandboxTemplate>),
            Wrapped { templates: Vec<SandboxTemplate> },
        }

        let parsed: TemplatesResponse = self.get_json("/templates").await?;
        Ok(match parsed {
            TemplatesResponse::Direct(items) => items,
            TemplatesResponse::Wrapped { templates } => templates,
        })
    }

    /// Delete a template by name.
    pub async fn delete_template(&self, name: &str) -> Result<(), SandboxError> {
        self.delete(&format!("/templates/{}", urlencoding::encode(name)))
            .await
    }

    // ── Volumes ──

    /// Create a new volume.
    pub async fn create_volume(&self, name: &str, size: &str) -> Result<Volume, SandboxError> {
        let body = serde_json::json!({ "name": name, "size": size });
        self.post_json("/volumes", &body).await
    }

    /// List all volumes.
    pub async fn list_volumes(&self) -> Result<Vec<Volume>, SandboxError> {
        self.get_json("/volumes").await
    }

    /// Delete a volume by name.
    pub async fn delete_volume(&self, name: &str) -> Result<(), SandboxError> {
        self.delete(&format!("/volumes/{}", urlencoding::encode(name)))
            .await
    }

    // ── Pools ──

    /// Create a new pool.
    pub async fn create_pool(
        &self,
        name: &str,
        template_name: &str,
        replicas: u32,
    ) -> Result<Pool, SandboxError> {
        let body = serde_json::json!({
            "name": name,
            "template_name": template_name,
            "replicas": replicas,
        });
        self.post_json("/pools", &body).await
    }

    /// List all pools.
    pub async fn list_pools(&self) -> Result<Vec<Pool>, SandboxError> {
        self.get_json("/pools").await
    }

    /// Delete a pool by name.
    pub async fn delete_pool(&self, name: &str) -> Result<(), SandboxError> {
        self.delete(&format!("/pools/{}", urlencoding::encode(name)))
            .await
    }
}
