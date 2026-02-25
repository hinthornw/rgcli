use super::config::Config;
use super::constants::{DEFAULT_IMAGE_DISTRO, DEFAULT_PYTHON_VERSION};

/// Get the default base image for a config.
pub fn default_base_image(config: &Config) -> String {
    if let Some(ref base) = config.base_image {
        return base.clone();
    }
    if config.node_version.is_some() && config.python_version.is_none() {
        "langchain/langgraphjs-api".to_string()
    } else {
        "langchain/langgraph-api".to_string()
    }
}

/// Build the Docker image tag string.
pub fn docker_tag(
    config: &Config,
    base_image: Option<&str>,
    api_version: Option<&str>,
) -> String {
    let api_version = api_version
        .map(|s| s.to_string())
        .or_else(|| config.api_version.clone());
    let base_image = base_image
        .map(|s| s.to_string())
        .unwrap_or_else(|| default_base_image(config));

    let image_distro = config
        .image_distro
        .as_deref()
        .unwrap_or(DEFAULT_IMAGE_DISTRO);
    let distro_tag = if image_distro == DEFAULT_IMAGE_DISTRO {
        String::new()
    } else {
        format!("-{image_distro}")
    };

    if let Some(ref tag) = config.internal_docker_tag {
        return format!("{base_image}:{tag}");
    }

    // Build the standard tag format
    let (language, version) = if config.node_version.is_some() && config.python_version.is_none() {
        ("node", config.node_version.as_deref().unwrap_or("20"))
    } else {
        (
            "py",
            config
                .python_version
                .as_deref()
                .unwrap_or(DEFAULT_PYTHON_VERSION),
        )
    };

    let version_distro_tag = format!("{version}{distro_tag}");

    if let Some(api_ver) = api_version {
        format!("{base_image}:{api_ver}-{language}{version_distro_tag}")
    } else if base_image.contains("/langgraph-server") && !base_image.contains(&version_distro_tag)
    {
        format!("{base_image}-{language}{version_distro_tag}")
    } else {
        format!("{base_image}:{version_distro_tag}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_image_python_only() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            node_version: None,
            base_image: None,
            ..Default::default()
        };
        assert_eq!(default_base_image(&config), "langchain/langgraph-api");
    }

    #[test]
    fn default_base_image_node_only() {
        let config = Config {
            python_version: None,
            node_version: Some("20".to_string()),
            base_image: None,
            ..Default::default()
        };
        assert_eq!(default_base_image(&config), "langchain/langgraphjs-api");
    }

    #[test]
    fn default_base_image_custom() {
        let config = Config {
            base_image: Some("custom/image".to_string()),
            ..Default::default()
        };
        assert_eq!(default_base_image(&config), "custom/image");
    }

    #[test]
    fn default_base_image_both_languages() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            node_version: Some("20".to_string()),
            base_image: None,
            ..Default::default()
        };
        // Python takes precedence when both are present
        assert_eq!(default_base_image(&config), "langchain/langgraph-api");
    }

    #[test]
    fn docker_tag_python_default() {
        let config = Config {
            python_version: None,
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:3.11");
    }

    #[test]
    fn docker_tag_python_custom_version() {
        let config = Config {
            python_version: Some("3.12".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:3.12");
    }

    #[test]
    fn docker_tag_node_only() {
        let config = Config {
            node_version: Some("20".to_string()),
            python_version: None,
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraphjs-api:20");
    }

    #[test]
    fn docker_tag_with_api_version() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, Some("0.1.0"));
        assert_eq!(tag, "langchain/langgraph-api:0.1.0-py3.11");
    }

    #[test]
    fn docker_tag_with_config_api_version() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            api_version: Some("0.2.0".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:0.2.0-py3.11");
    }

    #[test]
    fn docker_tag_api_version_override() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            api_version: Some("0.1.0".to_string()),
            ..Default::default()
        };
        // Parameter api_version should override config.api_version
        let tag = docker_tag(&config, None, Some("0.2.0"));
        assert_eq!(tag, "langchain/langgraph-api:0.2.0-py3.11");
    }

    #[test]
    fn docker_tag_custom_base_image() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            base_image: Some("custom/base".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "custom/base:3.11");
    }

    #[test]
    fn docker_tag_base_image_override() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            base_image: Some("config/base".to_string()),
            ..Default::default()
        };
        // Parameter base_image should override config.base_image
        let tag = docker_tag(&config, Some("override/base"), None);
        assert_eq!(tag, "override/base:3.11");
    }

    #[test]
    fn docker_tag_wolfi_distro() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            image_distro: Some("wolfi".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:3.11-wolfi");
    }

    #[test]
    fn docker_tag_debian_distro_no_suffix() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            image_distro: Some("debian".to_string()),
            ..Default::default()
        };
        // Debian is default, so no distro suffix
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:3.11");
    }

    #[test]
    fn docker_tag_bookworm_distro() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            image_distro: Some("bookworm".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:3.11-bookworm");
    }

    #[test]
    fn docker_tag_with_distro_and_api_version() {
        let config = Config {
            python_version: Some("3.12".to_string()),
            image_distro: Some("wolfi".to_string()),
            api_version: Some("0.1.0".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:0.1.0-py3.12-wolfi");
    }

    #[test]
    fn docker_tag_internal_tag_override() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            internal_docker_tag: Some("custom-tag".to_string()),
            ..Default::default()
        };
        // Internal docker tag should override all other tag logic
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraph-api:custom-tag");
    }

    #[test]
    fn docker_tag_internal_tag_with_custom_base() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            base_image: Some("custom/base".to_string()),
            internal_docker_tag: Some("special".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "custom/base:special");
    }

    #[test]
    fn docker_tag_langgraph_server_path() {
        let config = Config {
            python_version: Some("3.11".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, Some("registry/langgraph-server"), None);
        assert_eq!(tag, "registry/langgraph-server-py3.11");
    }

    #[test]
    fn docker_tag_node_with_distro() {
        let config = Config {
            node_version: Some("20".to_string()),
            python_version: None,
            image_distro: Some("wolfi".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraphjs-api:20-wolfi");
    }

    #[test]
    fn docker_tag_node_with_api_version() {
        let config = Config {
            node_version: Some("20".to_string()),
            python_version: None,
            api_version: Some("1.0.0".to_string()),
            ..Default::default()
        };
        let tag = docker_tag(&config, None, None);
        assert_eq!(tag, "langchain/langgraphjs-api:1.0.0-node20");
    }
}
