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
    depends.insert("langgraph-postgres".to_string(), YamlValue::Dict(pg_condition));
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
    redis_healthcheck.insert(
        "interval".to_string(),
        YamlValue::String("5s".to_string()),
    );
    redis_healthcheck.insert(
        "timeout".to_string(),
        YamlValue::String("1s".to_string()),
    );
    redis_healthcheck.insert(
        "retries".to_string(),
        YamlValue::String("5".to_string()),
    );
    redis.insert("healthcheck".to_string(), YamlValue::Dict(redis_healthcheck));
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
            YamlValue::List(vec![
                "langgraph-data:/var/lib/postgresql/data".to_string(),
            ]),
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
        pg_healthcheck.insert(
            "timeout".to_string(),
            YamlValue::String("1s".to_string()),
        );
        pg_healthcheck.insert(
            "retries".to_string(),
            YamlValue::String("5".to_string()),
        );

        if capabilities.healthcheck_start_interval {
            pg_healthcheck.insert(
                "interval".to_string(),
                YamlValue::String("60s".to_string()),
            );
            pg_healthcheck.insert(
                "start_interval".to_string(),
                YamlValue::String("1s".to_string()),
            );
        } else {
            pg_healthcheck.insert(
                "interval".to_string(),
                YamlValue::String("5s".to_string()),
            );
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
    api.insert("depends_on".to_string(), YamlValue::Dict(api_depends.clone()));

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
        api.insert(
            "image".to_string(),
            YamlValue::String(img.to_string()),
        );
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
        api_healthcheck.insert(
            "interval".to_string(),
            YamlValue::String("60s".to_string()),
        );
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
        vol_config.insert(
            "driver".to_string(),
            YamlValue::String("local".to_string()),
        );
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
