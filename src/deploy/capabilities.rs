use std::process::Command;

/// Semantic version tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

/// Type of Docker Compose installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeType {
    Plugin,
    Standalone,
}

/// Docker capabilities detected on the system.
#[derive(Debug, Clone)]
pub struct DockerCapabilities {
    #[allow(dead_code)]
    pub version_docker: Version,
    #[allow(dead_code)]
    pub version_compose: Version,
    pub healthcheck_start_interval: bool,
    pub compose_type: ComposeType,
}

/// Parse a version string like "1.2.3", "v1.2.3-alpha", etc.
pub fn parse_version(version: &str) -> Version {
    let cleaned = version.trim();
    let parts: Vec<&str> = cleaned.split('.').collect();

    let parse_part = |s: &str| -> u32 {
        let s = s.trim_start_matches('v');
        let s = s.split('-').next().unwrap_or(s);
        let s = s.split('+').next().unwrap_or(s);
        s.parse().unwrap_or(0)
    };

    match parts.len() {
        1 => Version::new(parse_part(parts[0]), 0, 0),
        2 => Version::new(parse_part(parts[0]), parse_part(parts[1]), 0),
        _ => Version::new(
            parse_part(parts[0]),
            parse_part(parts[1]),
            parse_part(parts[2]),
        ),
    }
}

/// Check Docker capabilities on the system.
pub fn check_capabilities() -> Result<DockerCapabilities, String> {
    // Check docker is available
    if which::which("docker").is_err() {
        return Err("Docker not installed".to_string());
    }

    // Get docker info
    let output = Command::new("docker")
        .args(["info", "-f", "{{json .}}"])
        .output()
        .map_err(|_| "Docker not installed or not running".to_string())?;

    if !output.status.success() {
        return Err("Docker not installed or not running".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let info: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|_| "Docker not installed or not running".to_string())?;

    let server_version = info
        .get("ServerVersion")
        .and_then(|v| v.as_str())
        .ok_or("Docker not running")?;

    if server_version.is_empty() {
        return Err("Docker not running".to_string());
    }

    // Try to find compose as plugin
    let (compose_version_str, compose_type) = if let Some(plugins) = info
        .get("ClientInfo")
        .and_then(|ci| ci.get("Plugins"))
        .and_then(|p| p.as_array())
    {
        if let Some(compose) = plugins
            .iter()
            .find(|p| p.get("Name").and_then(|n| n.as_str()) == Some("compose"))
        {
            let version = compose
                .get("Version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0");
            (version.to_string(), ComposeType::Plugin)
        } else {
            get_standalone_compose_version()?
        }
    } else {
        get_standalone_compose_version()?
    };

    let docker_version = parse_version(server_version);
    let compose_version = parse_version(&compose_version_str);

    Ok(DockerCapabilities {
        version_docker: docker_version,
        version_compose: compose_version,
        healthcheck_start_interval: docker_version >= Version::new(25, 0, 0),
        compose_type,
    })
}

fn get_standalone_compose_version() -> Result<(String, ComposeType), String> {
    if which::which("docker-compose").is_err() {
        return Err("Docker Compose not installed".to_string());
    }

    let output = Command::new("docker-compose")
        .args(["--version", "--short"])
        .output()
        .map_err(|_| "Docker Compose not installed".to_string())?;

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((version, ComposeType::Standalone))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_simple() {
        let v = parse_version("1.2.3");
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn parse_version_with_v_prefix() {
        let v = parse_version("v2.5.0");
        assert_eq!(v, Version::new(2, 5, 0));
    }

    #[test]
    fn parse_version_with_prerelease() {
        let v = parse_version("3.4.5-alpha");
        assert_eq!(v, Version::new(3, 4, 5));
    }

    #[test]
    fn parse_version_with_build_metadata() {
        let v = parse_version("1.2.3+build.123");
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn parse_version_with_both_prerelease_and_build() {
        let v = parse_version("v1.0.0-rc.1+build.456");
        assert_eq!(v, Version::new(1, 0, 0));
    }

    #[test]
    fn parse_version_major_only() {
        let v = parse_version("5");
        assert_eq!(v, Version::new(5, 0, 0));
    }

    #[test]
    fn parse_version_major_minor_only() {
        let v = parse_version("10.5");
        assert_eq!(v, Version::new(10, 5, 0));
    }

    #[test]
    fn parse_version_with_whitespace() {
        let v = parse_version("  1.2.3  ");
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn parse_version_invalid_fallback() {
        // Invalid version strings should parse to 0.0.0
        let v = parse_version("abc.def.ghi");
        assert_eq!(v, Version::new(0, 0, 0));
    }

    #[test]
    fn parse_version_partial_invalid() {
        // Partial invalid should parse what it can
        let v = parse_version("25.0.invalid");
        assert_eq!(v, Version::new(25, 0, 0));
    }

    #[test]
    fn version_comparison_equal() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 2, 3);
        assert_eq!(v1, v2);
    }

    #[test]
    fn version_comparison_greater() {
        let v1 = Version::new(2, 0, 0);
        let v2 = Version::new(1, 9, 9);
        assert!(v1 > v2);
    }

    #[test]
    fn version_comparison_less() {
        let v1 = Version::new(1, 2, 3);
        let v2 = Version::new(1, 2, 4);
        assert!(v1 < v2);
    }

    #[test]
    fn version_comparison_minor() {
        let v1 = Version::new(1, 5, 0);
        let v2 = Version::new(1, 3, 0);
        assert!(v1 > v2);
    }

    #[test]
    fn version_comparison_patch() {
        let v1 = Version::new(1, 2, 5);
        let v2 = Version::new(1, 2, 3);
        assert!(v1 > v2);
    }

    #[test]
    fn version_ordering() {
        let mut versions = vec![
            Version::new(2, 0, 0),
            Version::new(1, 0, 0),
            Version::new(1, 5, 0),
            Version::new(1, 0, 1),
        ];
        versions.sort();
        assert_eq!(
            versions,
            vec![
                Version::new(1, 0, 0),
                Version::new(1, 0, 1),
                Version::new(1, 5, 0),
                Version::new(2, 0, 0),
            ]
        );
    }

    #[test]
    fn compose_type_equality() {
        assert_eq!(ComposeType::Plugin, ComposeType::Plugin);
        assert_eq!(ComposeType::Standalone, ComposeType::Standalone);
        assert_ne!(ComposeType::Plugin, ComposeType::Standalone);
    }

    #[test]
    fn docker_capabilities_healthcheck_version_25() {
        let caps = DockerCapabilities {
            version_docker: Version::new(25, 0, 0),
            version_compose: Version::new(2, 20, 0),
            healthcheck_start_interval: true,
            compose_type: ComposeType::Plugin,
        };
        assert!(caps.healthcheck_start_interval);
    }

    #[test]
    fn docker_capabilities_healthcheck_version_24() {
        let caps = DockerCapabilities {
            version_docker: Version::new(24, 0, 0),
            version_compose: Version::new(2, 20, 0),
            healthcheck_start_interval: false,
            compose_type: ComposeType::Plugin,
        };
        assert!(!caps.healthcheck_start_interval);
    }

    #[test]
    fn docker_version_check_for_healthcheck() {
        let docker_version = Version::new(25, 0, 0);
        let min_version = Version::new(25, 0, 0);
        assert!(docker_version >= min_version);

        let docker_version_old = Version::new(24, 9, 9);
        assert!(docker_version_old < min_version);
    }

    #[test]
    fn parse_docker_24_version() {
        let v = parse_version("24.0.7");
        assert_eq!(v, Version::new(24, 0, 7));
        assert!(v < Version::new(25, 0, 0));
    }

    #[test]
    fn parse_docker_25_version() {
        let v = parse_version("25.0.0");
        assert_eq!(v, Version::new(25, 0, 0));
        assert!(v >= Version::new(25, 0, 0));
    }

    #[test]
    fn parse_compose_version_v2() {
        let v = parse_version("v2.23.0");
        assert_eq!(v, Version::new(2, 23, 0));
    }

    #[test]
    fn parse_compose_version_legacy() {
        let v = parse_version("1.29.2");
        assert_eq!(v, Version::new(1, 29, 2));
    }
}
