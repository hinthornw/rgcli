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
