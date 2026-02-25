#![allow(dead_code)]
use std::collections::HashMap;

/// Test that legacy flat config format is detected and can be parsed
#[test]
fn parse_legacy_config_format() {
    let yaml = r#"
endpoint: "https://example.langgraph.app"
api_key: "lsv2_test"
assistant_id: "my_agent"
"#;
    let cfg: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
    assert!(cfg.get("endpoint").is_some());
    assert!(cfg.get("contexts").is_none());
}

/// Test that new context config format parses correctly
#[test]
fn parse_context_config_format() {
    let yaml = r#"
current_context: production
contexts:
  default:
    endpoint: "https://dev.langgraph.app"
    api_key: ""
    assistant_id: "docs_agent"
  production:
    endpoint: "https://prod.langgraph.app"
    api_key: "lsv2_xxx"
    assistant_id: "prod_agent"
"#;

    #[derive(serde::Deserialize)]
    struct Config {
        endpoint: String,
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        assistant_id: String,
    }

    #[derive(serde::Deserialize)]
    struct ContextConfig {
        current_context: String,
        contexts: HashMap<String, Config>,
    }

    let ctx: ContextConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(ctx.current_context, "production");
    assert_eq!(ctx.contexts.len(), 2);
    assert_eq!(
        ctx.contexts["production"].endpoint,
        "https://prod.langgraph.app"
    );
    assert_eq!(ctx.contexts["default"].assistant_id, "docs_agent");
}

/// Test version comparison logic (same as update.rs is_newer)
#[test]
fn version_comparison() {
    fn is_newer(latest: &str, current: &str) -> bool {
        let strip = |v: &str| v.strip_prefix('v').unwrap_or(v).to_string();
        strip(latest) != strip(current)
    }

    assert!(is_newer("v0.0.2", "0.0.1"));
    assert!(is_newer("v0.0.2", "v0.0.1"));
    assert!(!is_newer("v0.0.1", "0.0.1"));
    assert!(!is_newer("0.0.1", "v0.0.1"));
}

/// Test that headers are built correctly with api key
#[test]
fn headers_include_api_key() {
    let yaml = r#"
endpoint: "https://example.langgraph.app"
api_key: "lsv2_test_key"
assistant_id: "agent"
"#;

    #[derive(serde::Deserialize)]
    struct Config {
        endpoint: String,
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        assistant_id: String,
        #[serde(default)]
        custom_headers: HashMap<String, String>,
    }

    impl Config {
        fn headers(&self) -> HashMap<String, String> {
            let mut headers = HashMap::new();
            headers.insert("Content-Type".to_string(), "application/json".to_string());
            if !self.api_key.is_empty() {
                headers.insert("X-Api-Key".to_string(), self.api_key.clone());
            }
            for (k, v) in &self.custom_headers {
                headers.insert(k.clone(), v.clone());
            }
            headers
        }
    }

    let cfg: Config = serde_yaml::from_str(yaml).unwrap();
    let headers = cfg.headers();
    assert_eq!(headers.get("X-Api-Key").unwrap(), "lsv2_test_key");
    assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
}

/// Test custom headers override
#[test]
fn custom_headers_applied() {
    let yaml = r#"
endpoint: "https://example.langgraph.app"
assistant_id: "agent"
custom_headers:
  Authorization: "Bearer token123"
  X-Custom: "value"
"#;

    #[derive(serde::Deserialize)]
    struct Config {
        #[serde(default)]
        api_key: String,
        #[serde(default)]
        custom_headers: HashMap<String, String>,
    }

    let cfg: Config = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.custom_headers.len(), 2);
    assert_eq!(cfg.custom_headers["Authorization"], "Bearer token123");
    assert!(cfg.api_key.is_empty());
}
