use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Result of executing a command in a sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub exit_code: i32,
}

impl ExecutionResult {
    /// Whether the command exited successfully (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Resource allocation for a sandbox template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    #[serde(default = "default_cpu")]
    pub cpu: String,
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default)]
    pub storage: Option<String>,
}

fn default_cpu() -> String {
    "500m".to_string()
}
fn default_memory() -> String {
    "512Mi".to_string()
}

impl Default for ResourceSpec {
    fn default() -> Self {
        Self {
            cpu: default_cpu(),
            memory: default_memory(),
            storage: None,
        }
    }
}

/// A volume mount specification for a sandbox template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMountSpec {
    pub volume_name: String,
    pub mount_path: String,
}

/// A persistent volume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub name: String,
    pub size: String,
    #[serde(default)]
    pub storage_class: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// A sandbox template defining the container image and resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxTemplate {
    pub name: String,
    pub image: String,
    #[serde(default)]
    pub resources: ResourceSpec,
    #[serde(default)]
    pub volume_mounts: Vec<VolumeMountSpec>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// A pool of pre-warmed sandbox instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    pub name: String,
    pub template_name: String,
    #[serde(default)]
    pub replicas: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Specification for creating a new template.
#[derive(Debug, Clone, Serialize)]
pub struct CreateTemplate {
    pub name: String,
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_mounts: Option<Vec<VolumeMountSpec>>,
}

/// Options for running a command in a sandbox.
#[derive(Debug, Clone)]
pub struct RunOpts {
    pub command: String,
    pub timeout: u64,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    pub shell: String,
}

impl RunOpts {
    pub fn new(command: &str) -> Self {
        Self {
            command: command.to_string(),
            timeout: 60,
            env: HashMap::new(),
            cwd: None,
            shell: "/bin/bash".to_string(),
        }
    }

    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = secs;
        self
    }

    pub fn env(mut self, key: &str, val: &str) -> Self {
        self.env.insert(key.to_string(), val.to_string());
        self
    }

    pub fn cwd(mut self, dir: &str) -> Self {
        self.cwd = Some(dir.to_string());
        self
    }

    pub fn shell(mut self, shell: &str) -> Self {
        self.shell = shell.to_string();
        self
    }
}

/// A chunk of streaming output from a WebSocket command execution.
#[derive(Debug, Clone)]
pub struct OutputChunk {
    /// Either "stdout" or "stderr".
    pub stream: String,
    /// The text content of this chunk.
    pub data: String,
    /// Byte offset within the stream (used for reconnection).
    pub offset: usize,
}

/// Sandbox metadata returned from control plane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub name: String,
    #[serde(default)]
    pub template_name: String,
    #[serde(default)]
    pub dataplane_url: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_result_success() {
        let ok = ExecutionResult {
            stdout: "hello".into(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert!(ok.success());

        let fail = ExecutionResult {
            stdout: String::new(),
            stderr: "err".into(),
            exit_code: 1,
        };
        assert!(!fail.success());
    }

    #[test]
    fn run_opts_builder() {
        let opts = RunOpts::new("ls -la")
            .timeout(30)
            .env("FOO", "bar")
            .cwd("/tmp")
            .shell("/bin/sh");
        assert_eq!(opts.command, "ls -la");
        assert_eq!(opts.timeout, 30);
        assert_eq!(opts.env.get("FOO").unwrap(), "bar");
        assert_eq!(opts.cwd.as_deref(), Some("/tmp"));
        assert_eq!(opts.shell, "/bin/sh");
    }

    #[test]
    fn deserialize_execution_result() {
        let json = r#"{"stdout":"hello\n","stderr":"","exit_code":0}"#;
        let result: ExecutionResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert!(result.success());
    }

    #[test]
    fn deserialize_sandbox_info() {
        let json = r#"{
            "name": "test-sb",
            "template_name": "python",
            "dataplane_url": "https://dp.example.com",
            "id": "abc-123"
        }"#;
        let info: SandboxInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "test-sb");
        assert_eq!(
            info.dataplane_url.as_deref(),
            Some("https://dp.example.com")
        );
    }

    #[test]
    fn resource_spec_defaults() {
        let spec = ResourceSpec::default();
        assert_eq!(spec.cpu, "500m");
        assert_eq!(spec.memory, "512Mi");
        assert!(spec.storage.is_none());
    }
}
