use indexmap::IndexMap;

use super::capabilities::DockerCapabilities;
use super::constants::DEFAULT_POSTGRES_URI;

/// Value types for our custom YAML writer.
#[derive(Debug, Clone)]
pub enum YamlValue {
    String(String),
    Dict(IndexMap<String, YamlValue>),
    List(Vec<String>),
}

/// Convert a dictionary to a YAML string with custom formatting.
pub fn dict_to_yaml(d: &IndexMap<String, YamlValue>, indent: usize) -> String {
    let mut yaml_str = String::new();

    for (idx, (key, value)) in d.iter().enumerate() {
        // Extra newline for top-level keys only (after the first)
        if idx >= 1 && indent < 2 {
            yaml_str.push('\n');
        }
        let space = "    ".repeat(indent);
        match value {
            YamlValue::Dict(inner) => {
                yaml_str.push_str(&format!("{space}{key}:\n"));
                yaml_str.push_str(&dict_to_yaml(inner, indent + 1));
            }
            YamlValue::List(items) => {
                yaml_str.push_str(&format!("{space}{key}:\n"));
                for item in items {
                    yaml_str.push_str(&format!("{space}    - {item}\n"));
                }
            }
            YamlValue::String(val) => {
                yaml_str.push_str(&format!("{space}{key}: {val}\n"));
            }
        }
    }
    yaml_str
}

/// Create debugger service config.
pub fn debugger_compose(port: Option<u16>, base_url: Option<&str>) -> IndexMap<String, YamlValue> {
    let port = match port {
        Some(p) => p,
        None => return IndexMap::new(),
    };

    let mut debugger = IndexMap::new();
    debugger.insert(
        "image".to_string(),
        YamlValue::String("langchain/langgraph-debugger".to_string()),
    );
    debugger.insert(
        "restart".to_string(),
        YamlValue::String("on-failure".to_string()),
    );

    let mut depends = IndexMap::new();
    let mut pg_condition = IndexMap::new();
    pg_condition.insert(
        "condition".to_string(),
        YamlValue::String("service_healthy".to_string()),
    );
    depends.insert(
        "langgraph-postgres".to_string(),
        YamlValue::Dict(pg_condition),
    );
    debugger.insert("depends_on".to_string(), YamlValue::Dict(depends));

    debugger.insert(
        "ports".to_string(),
        YamlValue::List(vec![format!("\"{port}:3968\"")]),
    );

    if let Some(url) = base_url {
        let mut env = IndexMap::new();
        env.insert(
            "VITE_STUDIO_LOCAL_GRAPH_URL".to_string(),
            YamlValue::String(url.to_string()),
        );
        debugger.insert("environment".to_string(), YamlValue::Dict(env));
    }

    let mut result = IndexMap::new();
    result.insert("langgraph-debugger".to_string(), YamlValue::Dict(debugger));
    result
}

/// Create a docker compose file as a dictionary.
#[allow(clippy::too_many_arguments)]
pub fn compose_as_dict(
    capabilities: &DockerCapabilities,
    port: u16,
    debugger_port: Option<u16>,
    debugger_base_url: Option<&str>,
    postgres_uri: Option<&str>,
    image: Option<&str>,
    _base_image: Option<&str>,
    _api_version: Option<&str>,
) -> IndexMap<String, YamlValue> {
    let include_db = postgres_uri.is_none();
    let postgres_uri = postgres_uri.unwrap_or(DEFAULT_POSTGRES_URI);

    let mut services = IndexMap::new();

    // Redis service
    let mut redis = IndexMap::new();
    redis.insert(
        "image".to_string(),
        YamlValue::String("redis:6".to_string()),
    );
    let mut redis_healthcheck = IndexMap::new();
    redis_healthcheck.insert(
        "test".to_string(),
        YamlValue::String("redis-cli ping".to_string()),
    );
    redis_healthcheck.insert("interval".to_string(), YamlValue::String("5s".to_string()));
    redis_healthcheck.insert("timeout".to_string(), YamlValue::String("1s".to_string()));
    redis_healthcheck.insert("retries".to_string(), YamlValue::String("5".to_string()));
    redis.insert(
        "healthcheck".to_string(),
        YamlValue::Dict(redis_healthcheck),
    );
    services.insert("langgraph-redis".to_string(), YamlValue::Dict(redis));

    // Postgres service (if needed)
    if include_db {
        let mut postgres = IndexMap::new();
        postgres.insert(
            "image".to_string(),
            YamlValue::String("pgvector/pgvector:pg16".to_string()),
        );
        postgres.insert(
            "ports".to_string(),
            YamlValue::List(vec!["\"5433:5432\"".to_string()]),
        );

        let mut pg_env = IndexMap::new();
        pg_env.insert(
            "POSTGRES_DB".to_string(),
            YamlValue::String("postgres".to_string()),
        );
        pg_env.insert(
            "POSTGRES_USER".to_string(),
            YamlValue::String("postgres".to_string()),
        );
        pg_env.insert(
            "POSTGRES_PASSWORD".to_string(),
            YamlValue::String("postgres".to_string()),
        );
        postgres.insert("environment".to_string(), YamlValue::Dict(pg_env));

        postgres.insert(
            "command".to_string(),
            YamlValue::List(vec![
                "postgres".to_string(),
                "-c".to_string(),
                "shared_preload_libraries=vector".to_string(),
            ]),
        );

        postgres.insert(
            "volumes".to_string(),
            YamlValue::List(vec!["langgraph-data:/var/lib/postgresql/data".to_string()]),
        );

        let mut pg_healthcheck = IndexMap::new();
        pg_healthcheck.insert(
            "test".to_string(),
            YamlValue::String("pg_isready -U postgres".to_string()),
        );
        pg_healthcheck.insert(
            "start_period".to_string(),
            YamlValue::String("10s".to_string()),
        );
        pg_healthcheck.insert("timeout".to_string(), YamlValue::String("1s".to_string()));
        pg_healthcheck.insert("retries".to_string(), YamlValue::String("5".to_string()));

        if capabilities.healthcheck_start_interval {
            pg_healthcheck.insert("interval".to_string(), YamlValue::String("60s".to_string()));
            pg_healthcheck.insert(
                "start_interval".to_string(),
                YamlValue::String("1s".to_string()),
            );
        } else {
            pg_healthcheck.insert("interval".to_string(), YamlValue::String("5s".to_string()));
        }

        postgres.insert("healthcheck".to_string(), YamlValue::Dict(pg_healthcheck));
        services.insert("langgraph-postgres".to_string(), YamlValue::Dict(postgres));
    }

    // Debugger service (if port specified)
    if let Some(dbg_port) = debugger_port {
        let debugger = debugger_compose(Some(dbg_port), debugger_base_url);
        for (k, v) in debugger {
            services.insert(k, v);
        }
    }

    // LangGraph API service
    let mut api = IndexMap::new();
    api.insert(
        "ports".to_string(),
        YamlValue::List(vec![format!("\"{port}:8000\"")]),
    );

    let mut api_depends = IndexMap::new();
    let mut redis_condition = IndexMap::new();
    redis_condition.insert(
        "condition".to_string(),
        YamlValue::String("service_healthy".to_string()),
    );
    api_depends.insert(
        "langgraph-redis".to_string(),
        YamlValue::Dict(redis_condition),
    );
    api.insert(
        "depends_on".to_string(),
        YamlValue::Dict(api_depends.clone()),
    );

    let mut api_env = IndexMap::new();
    api_env.insert(
        "REDIS_URI".to_string(),
        YamlValue::String("redis://langgraph-redis:6379".to_string()),
    );
    api_env.insert(
        "POSTGRES_URI".to_string(),
        YamlValue::String(postgres_uri.to_string()),
    );
    api.insert("environment".to_string(), YamlValue::Dict(api_env));

    if let Some(img) = image {
        api.insert("image".to_string(), YamlValue::String(img.to_string()));
    }

    // Add postgres dependency for API service
    if include_db {
        if let YamlValue::Dict(deps) = api.get_mut("depends_on").unwrap() {
            let mut pg_condition = IndexMap::new();
            pg_condition.insert(
                "condition".to_string(),
                YamlValue::String("service_healthy".to_string()),
            );
            deps.insert(
                "langgraph-postgres".to_string(),
                YamlValue::Dict(pg_condition),
            );
        }
    }

    // Healthcheck for API service
    if capabilities.healthcheck_start_interval {
        let mut api_healthcheck = IndexMap::new();
        api_healthcheck.insert(
            "test".to_string(),
            YamlValue::String("python /api/healthcheck.py".to_string()),
        );
        api_healthcheck.insert("interval".to_string(), YamlValue::String("60s".to_string()));
        api_healthcheck.insert(
            "start_interval".to_string(),
            YamlValue::String("1s".to_string()),
        );
        api_healthcheck.insert(
            "start_period".to_string(),
            YamlValue::String("10s".to_string()),
        );
        api.insert("healthcheck".to_string(), YamlValue::Dict(api_healthcheck));
    }

    services.insert("langgraph-api".to_string(), YamlValue::Dict(api));

    // Build final compose dict
    let mut compose_dict = IndexMap::new();
    if include_db {
        let mut volumes = IndexMap::new();
        let mut vol_config = IndexMap::new();
        vol_config.insert("driver".to_string(), YamlValue::String("local".to_string()));
        volumes.insert("langgraph-data".to_string(), YamlValue::Dict(vol_config));
        compose_dict.insert("volumes".to_string(), YamlValue::Dict(volumes));
    }
    compose_dict.insert("services".to_string(), YamlValue::Dict(services));

    compose_dict
}

/// Create a docker compose file as a string.
#[allow(clippy::too_many_arguments)]
pub fn compose(
    capabilities: &DockerCapabilities,
    port: u16,
    debugger_port: Option<u16>,
    debugger_base_url: Option<&str>,
    postgres_uri: Option<&str>,
    image: Option<&str>,
    base_image: Option<&str>,
    api_version: Option<&str>,
) -> String {
    let compose_dict = compose_as_dict(
        capabilities,
        port,
        debugger_port,
        debugger_base_url,
        postgres_uri,
        image,
        base_image,
        api_version,
    );
    dict_to_yaml(&compose_dict, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_capabilities(healthcheck_start_interval: bool) -> DockerCapabilities {
        DockerCapabilities {
            version_docker: if healthcheck_start_interval {
                super::super::capabilities::Version::new(25, 0, 0)
            } else {
                super::super::capabilities::Version::new(24, 0, 0)
            },
            version_compose: super::super::capabilities::Version::new(2, 20, 0),
            healthcheck_start_interval,
            compose_type: super::super::capabilities::ComposeType::Plugin,
        }
    }

    #[test]
    fn dict_to_yaml_simple() {
        let mut dict = IndexMap::new();
        dict.insert("key".to_string(), YamlValue::String("value".to_string()));
        let yaml = dict_to_yaml(&dict, 0);
        assert_eq!(yaml, "key: value\n");
    }

    #[test]
    fn dict_to_yaml_nested() {
        let mut inner = IndexMap::new();
        inner.insert(
            "inner_key".to_string(),
            YamlValue::String("inner_value".to_string()),
        );
        let mut dict = IndexMap::new();
        dict.insert("outer".to_string(), YamlValue::Dict(inner));
        let yaml = dict_to_yaml(&dict, 0);
        assert!(yaml.contains("outer:\n"));
        assert!(yaml.contains("    inner_key: inner_value\n"));
    }

    #[test]
    fn dict_to_yaml_list() {
        let mut dict = IndexMap::new();
        dict.insert(
            "items".to_string(),
            YamlValue::List(vec!["item1".to_string(), "item2".to_string()]),
        );
        let yaml = dict_to_yaml(&dict, 0);
        assert!(yaml.contains("items:\n"));
        assert!(yaml.contains("    - item1\n"));
        assert!(yaml.contains("    - item2\n"));
    }

    #[test]
    fn debugger_compose_none() {
        let result = debugger_compose(None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn debugger_compose_with_port() {
        let result = debugger_compose(Some(8123), None);
        assert!(result.contains_key("langgraph-debugger"));

        if let Some(YamlValue::Dict(debugger)) = result.get("langgraph-debugger") {
            assert!(debugger.contains_key("image"));
            assert!(debugger.contains_key("ports"));
            assert!(debugger.contains_key("depends_on"));

            if let Some(YamlValue::List(ports)) = debugger.get("ports") {
                assert_eq!(ports.len(), 1);
                assert!(ports[0].contains("8123"));
            }
        } else {
            panic!("Expected debugger service dict");
        }
    }

    #[test]
    fn debugger_compose_with_base_url() {
        let result = debugger_compose(Some(8123), Some("http://localhost:8000"));

        if let Some(YamlValue::Dict(debugger)) = result.get("langgraph-debugger") {
            assert!(debugger.contains_key("environment"));

            if let Some(YamlValue::Dict(env)) = debugger.get("environment") {
                assert!(env.contains_key("VITE_STUDIO_LOCAL_GRAPH_URL"));
            }
        } else {
            panic!("Expected debugger service dict");
        }
    }

    #[test]
    fn compose_basic_services() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Check for required services
        assert!(yaml.contains("services:"));
        assert!(yaml.contains("langgraph-redis:"));
        assert!(yaml.contains("langgraph-postgres:"));
        assert!(yaml.contains("langgraph-api:"));

        // Check Redis configuration
        assert!(yaml.contains("image: redis:6"));
        assert!(yaml.contains("redis-cli ping"));

        // Check port binding
        assert!(yaml.contains("\"8000:8000\""));
    }

    #[test]
    fn compose_with_custom_port() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 9000, None, None, None, None, None, None);
        assert!(yaml.contains("\"9000:8000\""));
    }

    #[test]
    fn compose_with_custom_image() {
        let caps = mock_capabilities(false);
        let yaml = compose(
            &caps,
            8000,
            None,
            None,
            None,
            Some("custom/image:tag"),
            None,
            None,
        );
        assert!(yaml.contains("image: custom/image:tag"));
    }

    #[test]
    fn compose_with_external_postgres() {
        let caps = mock_capabilities(false);
        let yaml = compose(
            &caps,
            8000,
            None,
            None,
            Some("postgres://external:5432/db"),
            None,
            None,
            None,
        );

        // Should not include postgres service
        assert!(!yaml.contains("langgraph-postgres:"));
        assert!(!yaml.contains("pgvector/pgvector"));

        // Should not include volumes
        assert!(!yaml.contains("volumes:"));
        assert!(!yaml.contains("langgraph-data"));

        // Should include external URI in environment
        assert!(yaml.contains("postgres://external:5432/db"));
    }

    #[test]
    fn compose_with_debugger() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, Some(8123), None, None, None, None, None);

        assert!(yaml.contains("langgraph-debugger:"));
        assert!(yaml.contains("\"8123:3968\""));
        assert!(yaml.contains("langchain/langgraph-debugger"));
    }

    #[test]
    fn compose_healthcheck_with_start_interval() {
        let caps = mock_capabilities(true);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Docker 25+ should use start_interval
        assert!(yaml.contains("start_interval: 1s"));
        assert!(yaml.contains("interval: 60s"));
    }

    #[test]
    fn compose_healthcheck_without_start_interval() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Docker 24 should not use start_interval
        assert!(!yaml.contains("start_interval"));
        assert!(yaml.contains("interval: 5s"));
    }

    #[test]
    fn compose_postgres_configuration() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Check postgres service details
        assert!(yaml.contains("pgvector/pgvector:pg16"));
        assert!(yaml.contains("POSTGRES_DB: postgres"));
        assert!(yaml.contains("POSTGRES_USER: postgres"));
        assert!(yaml.contains("POSTGRES_PASSWORD: postgres"));
        assert!(yaml.contains("\"5433:5432\""));
        assert!(yaml.contains("pg_isready -U postgres"));
        assert!(yaml.contains("shared_preload_libraries=vector"));
    }

    #[test]
    fn compose_api_dependencies() {
        let caps = mock_capabilities(false);
        let dict = compose_as_dict(&caps, 8000, None, None, None, None, None, None);

        if let Some(YamlValue::Dict(services)) = dict.get("services") {
            if let Some(YamlValue::Dict(api)) = services.get("langgraph-api") {
                if let Some(YamlValue::Dict(depends_on)) = api.get("depends_on") {
                    // Should depend on both redis and postgres
                    assert!(depends_on.contains_key("langgraph-redis"));
                    assert!(depends_on.contains_key("langgraph-postgres"));
                } else {
                    panic!("Expected depends_on dict");
                }
            } else {
                panic!("Expected langgraph-api service");
            }
        } else {
            panic!("Expected services dict");
        }
    }

    #[test]
    fn compose_api_environment() {
        let caps = mock_capabilities(false);
        let dict = compose_as_dict(&caps, 8000, None, None, None, None, None, None);

        if let Some(YamlValue::Dict(services)) = dict.get("services") {
            if let Some(YamlValue::Dict(api)) = services.get("langgraph-api") {
                if let Some(YamlValue::Dict(env)) = api.get("environment") {
                    assert!(env.contains_key("REDIS_URI"));
                    assert!(env.contains_key("POSTGRES_URI"));
                } else {
                    panic!("Expected environment dict");
                }
            } else {
                panic!("Expected langgraph-api service");
            }
        } else {
            panic!("Expected services dict");
        }
    }

    #[test]
    fn compose_volumes_included_with_postgres() {
        let caps = mock_capabilities(false);
        let dict = compose_as_dict(&caps, 8000, None, None, None, None, None, None);

        // With embedded postgres, should have volumes
        assert!(dict.contains_key("volumes"));
    }

    #[test]
    fn compose_volumes_excluded_without_postgres() {
        let caps = mock_capabilities(false);
        let dict = compose_as_dict(
            &caps,
            8000,
            None,
            None,
            Some("postgres://external:5432/db"),
            None,
            None,
            None,
        );

        // With external postgres, should not have volumes
        assert!(!dict.contains_key("volumes"));
    }

    #[test]
    fn compose_yaml_format() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Check that YAML has proper structure with blank lines between top-level keys
        let lines: Vec<&str> = yaml.lines().collect();

        // Should start with volumes or services
        assert!(lines[0].starts_with("volumes:") || lines[0].starts_with("services:"));

        // Should have proper indentation (4 spaces)
        let indented_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with("    ")).collect();
        assert!(!indented_lines.is_empty());
    }

    #[test]
    fn compose_redis_healthcheck() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        assert!(yaml.contains("test: redis-cli ping"));
        assert!(yaml.contains("interval: 5s"));
        assert!(yaml.contains("timeout: 1s"));
        assert!(yaml.contains("retries: 5"));
    }

    #[test]
    fn compose_api_healthcheck_docker_25() {
        let caps = mock_capabilities(true);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Should have healthcheck for API service with Docker 25+
        assert!(yaml.contains("python /api/healthcheck.py"));
        assert!(yaml.contains("start_period: 10s"));
    }

    #[test]
    fn compose_api_no_healthcheck_docker_24() {
        let caps = mock_capabilities(false);
        let yaml = compose(&caps, 8000, None, None, None, None, None, None);

        // Should not have API healthcheck with Docker 24
        assert!(!yaml.contains("python /api/healthcheck.py"));
    }
}
