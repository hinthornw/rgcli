use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use super::config::{Config, EnvConfig, KeepPkgTools};
use super::constants::{BUILD_TOOLS, DEFAULT_NODE_VERSION};
use super::docker_tag::docker_tag;
use super::local_deps::{assemble_local_deps, LocalDeps};
use super::path_rewrite::{
    update_auth_path, update_encryption_path, update_graph_paths, update_http_app_path,
};

static IMAGE_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":(\d+(?:\.\d+)?(?:\.\d+)?)(?:-|$)").unwrap());

/// Check if a base image supports uv.
fn image_supports_uv(base_image: &str) -> bool {
    if base_image == "langchain/langgraph-trial" {
        return false;
    }
    match IMAGE_VERSION_RE.find(base_image) {
        None => true, // Default image supports it
        Some(m) => {
            let raw = &base_image[m.start() + 1..m.end()];
            let version_str = raw.trim_end_matches('-');
            let parts: Vec<u32> = version_str.split('.').filter_map(|p: &str| p.parse().ok()).collect();
            let version = (
                *parts.first().unwrap_or(&0),
                *parts.get(1).unwrap_or(&0),
                *parts.get(2).unwrap_or(&0),
            );
            version >= (0, 2, 47)
        }
    }
}

/// Get build tools to uninstall based on config.
fn get_build_tools_to_uninstall(config: &Config) -> Vec<String> {
    match &config.keep_pkg_tools {
        None => BUILD_TOOLS.iter().map(|s| s.to_string()).collect(),
        Some(KeepPkgTools::Bool(true)) => vec![],
        Some(KeepPkgTools::Bool(false)) => BUILD_TOOLS.iter().map(|s| s.to_string()).collect(),
        Some(KeepPkgTools::List(keep)) => {
            let keep_set: HashSet<&str> = keep.iter().map(|s| s.as_str()).collect();
            BUILD_TOOLS
                .iter()
                .filter(|t| !keep_set.contains(*t))
                .map(|s| s.to_string())
                .collect()
        }
    }
}

/// Generate pip cleanup lines for the Dockerfile.
fn get_pip_cleanup_lines(
    install_cmd: &str,
    to_uninstall: &[String],
    pip_installer: &str,
) -> String {
    let mut commands = vec![format!(
        "# -- Ensure user deps didn't inadvertently overwrite langgraph-api\n\
         RUN mkdir -p /api/langgraph_api /api/langgraph_runtime /api/langgraph_license && \\\n\
         touch /api/langgraph_api/__init__.py /api/langgraph_runtime/__init__.py /api/langgraph_license/__init__.py\n\
         RUN PYTHONDONTWRITEBYTECODE=1 {install_cmd} --no-cache-dir --no-deps -e /api\n\
         # -- End of ensuring user deps didn't inadvertently overwrite langgraph-api --\n\
         # -- Removing build deps from the final image ~<:===~~~ --"
    )];

    if !to_uninstall.is_empty() {
        let mut sorted = to_uninstall.to_vec();
        sorted.sort();
        let packs_str = sorted.join(" ");
        commands.push(format!("RUN pip uninstall -y {packs_str}"));

        let packages_rm: String = to_uninstall
            .iter()
            .map(|p| format!("/usr/local/lib/python*/site-packages/{p}*"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut rm_cmd = format!("RUN rm -rf {packages_rm}");
        if to_uninstall.contains(&"pip".to_string()) {
            rm_cmd.push_str(" && find /usr/local/bin -name \"pip*\" -delete || true");
        }
        commands.push(rm_cmd);

        let wolfi_rm: String = to_uninstall
            .iter()
            .map(|p| format!("/usr/lib/python*/site-packages/{p}*"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut wolfi_cmd = format!("RUN rm -rf {wolfi_rm}");
        if to_uninstall.contains(&"pip".to_string()) {
            wolfi_cmd.push_str(" && find /usr/bin -name \"pip*\" -delete || true");
        }
        commands.push(wolfi_cmd);

        if pip_installer == "uv" {
            commands.push(format!(
                "RUN uv pip uninstall --system {packs_str} && rm /usr/bin/uv /usr/bin/uvx"
            ));
        }
    } else if pip_installer == "uv" {
        commands.push(
            "RUN rm /usr/bin/uv /usr/bin/uvx\n# -- End of build deps removal --".to_string(),
        );
    }

    commands.join("\n")
}

/// Build ENV lines for all config-driven environment variables (shared between Python and Node).
fn build_config_env_vars(config: &Config) -> Vec<String> {
    let mut env_vars = Vec::new();

    if let Some(ref store) = config.store {
        env_vars.push(format!(
            "ENV LANGGRAPH_STORE='{}'",
            serde_json::to_string(store).unwrap()
        ));
    }
    if let Some(ref auth) = config.auth {
        env_vars.push(format!(
            "ENV LANGGRAPH_AUTH='{}'",
            serde_json::to_string(auth).unwrap()
        ));
    }
    if let Some(ref encryption) = config.encryption {
        env_vars.push(format!(
            "ENV LANGGRAPH_ENCRYPTION='{}'",
            serde_json::to_string(encryption).unwrap()
        ));
    }
    if let Some(ref http) = config.http {
        env_vars.push(format!(
            "ENV LANGGRAPH_HTTP='{}'",
            serde_json::to_string(http).unwrap()
        ));
    }
    if let Some(ref webhooks) = config.webhooks {
        env_vars.push(format!(
            "ENV LANGGRAPH_WEBHOOKS='{}'",
            serde_json::to_string(webhooks).unwrap()
        ));
    }
    if let Some(ref checkpointer) = config.checkpointer {
        env_vars.push(format!(
            "ENV LANGGRAPH_CHECKPOINTER='{}'",
            serde_json::to_string(checkpointer).unwrap()
        ));
    }
    if let Some(ref ui) = config.ui {
        env_vars.push(format!(
            "ENV LANGGRAPH_UI='{}'",
            serde_json::to_string(ui).unwrap()
        ));
    }
    if let Some(ref ui_config) = config.ui_config {
        env_vars.push(format!(
            "ENV LANGGRAPH_UI_CONFIG='{}'",
            serde_json::to_string(ui_config).unwrap()
        ));
    }
    env_vars.push(format!(
        "ENV LANGSERVE_GRAPHS='{}'",
        serde_json::to_string(&config.graphs).unwrap()
    ));

    env_vars
}

/// Detect the Node.js package manager install command.
fn get_node_pm_install_cmd(config_path: &Path, _config: &Config) -> String {
    let parent = config_path.parent().unwrap();

    let test_file = |name: &str| -> bool { parent.join(name).is_file() };

    let npm = test_file("package-lock.json");
    let yarn = test_file("yarn.lock");
    let pnpm = test_file("pnpm-lock.yaml");
    let bun = test_file("bun.lockb");

    if yarn {
        "yarn install --frozen-lockfile".to_string()
    } else if pnpm {
        "pnpm i --frozen-lockfile".to_string()
    } else if npm {
        "npm ci".to_string()
    } else if bun {
        "bun i".to_string()
    } else {
        // Try to detect from package.json
        let pkg_manager = get_pkg_manager_name(parent);
        match pkg_manager.as_deref() {
            Some("yarn") => "yarn install".to_string(),
            Some("pnpm") => "pnpm i".to_string(),
            Some("bun") => "bun i".to_string(),
            _ => "npm i".to_string(),
        }
    }
}

fn get_pkg_manager_name(dir: &Path) -> Option<String> {
    let pkg_path = dir.join("package.json");
    let content = std::fs::read_to_string(pkg_path).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Check packageManager field
    if let Some(pm) = pkg.get("packageManager").and_then(|v| v.as_str()) {
        let name = pm.trim_start_matches('^').split('@').next()?;
        return Some(name.to_string());
    }

    // Check devEngines.packageManager.name
    if let Some(name) = pkg
        .get("devEngines")
        .and_then(|de| de.get("packageManager"))
        .and_then(|pm| pm.get("name"))
        .and_then(|n| n.as_str())
    {
        return Some(name.to_string());
    }

    None
}

/// Generate a Dockerfile for a Python configuration.
pub fn python_config_to_docker(
    config_path: &Path,
    config: &mut Config,
    base_image: &str,
    api_version: Option<&str>,
    escape_variables: bool,
) -> Result<(String, IndexMap<String, String>), String> {
    let build_tools_to_uninstall = get_build_tools_to_uninstall(config);

    let pip_installer_raw = config
        .pip_installer
        .as_deref()
        .unwrap_or("auto")
        .to_string();
    let pip_installer: String = if pip_installer_raw == "auto" {
        if image_supports_uv(base_image) {
            "uv".to_string()
        } else {
            "pip".to_string()
        }
    } else {
        pip_installer_raw
    };

    let install_cmd = if pip_installer == "uv" {
        "uv pip install --system"
    } else {
        "pip install"
    };

    // Configure pip
    let local_reqs_pip_install = {
        let base = format!("PYTHONDONTWRITEBYTECODE=1 {install_cmd} --no-cache-dir -c /api/constraints.txt");
        if config.pip_config_file.is_some() {
            format!("PIP_CONFIG_FILE=/pipconfig.txt {base}")
        } else {
            base
        }
    };
    let global_reqs_pip_install = {
        let base = format!("PYTHONDONTWRITEBYTECODE=1 {install_cmd} --no-cache-dir -c /api/constraints.txt");
        if config.pip_config_file.is_some() {
            format!("PIP_CONFIG_FILE=/pipconfig.txt {base}")
        } else {
            base
        }
    };

    let pip_config_file_str = config
        .pip_config_file
        .as_ref()
        .map(|f| format!("ADD {f} /pipconfig.txt"))
        .unwrap_or_default();

    // Collect dependencies (owned to avoid borrowing config across mutable calls)
    let pypi_deps: Vec<String> = config
        .dependencies
        .iter()
        .filter(|dep| !dep.starts_with('.'))
        .cloned()
        .collect();

    let local_deps = assemble_local_deps(config_path, config)?;

    // Rewrite paths
    update_graph_paths(config_path, config, &local_deps)?;
    update_auth_path(config_path, config, &local_deps)?;
    update_encryption_path(config_path, config, &local_deps)?;
    update_http_app_path(config_path, config, &local_deps)?;

    let pip_pkgs_str = if !pypi_deps.is_empty() {
        format!("RUN {local_reqs_pip_install} {}", pypi_deps.join(" "))
    } else {
        String::new()
    };

    // Build pip requirements string
    let pip_reqs_str = build_pip_reqs_str(&local_deps, config_path, &local_reqs_pip_install);

    // Build faux packages string
    let faux_pkgs_str = build_faux_pkgs_str(&local_deps, config_path);

    // Build local packages string
    let local_pkgs_str = build_local_pkgs_str(&local_deps, config_path);

    // Install Node.js if needed
    let install_node_str = if (config.ui.is_some() || config.node_version.is_some())
        && local_deps.working_dir.is_some()
    {
        "RUN /storage/install-node.sh".to_string()
    } else {
        String::new()
    };

    // Combine install sections
    let installs: Vec<&str> = [
        &install_node_str,
        &pip_config_file_str,
        &pip_pkgs_str,
        &pip_reqs_str,
        &local_pkgs_str,
        &faux_pkgs_str,
    ]
    .iter()
    .filter(|s| !s.is_empty())
    .map(|s| s.as_str())
    .collect();
    let installs = installs.join("\n\n");

    // Environment variables
    let env_vars = build_config_env_vars(config);

    // JS install
    let js_inst_str =
        if (config.ui.is_some() || config.node_version.is_some()) && local_deps.working_dir.is_some()
        {
            let node_version = config
                .node_version
                .as_deref()
                .unwrap_or(DEFAULT_NODE_VERSION);
            let install_cmd_node = get_node_pm_install_cmd(config_path, config);
            let wd = local_deps.working_dir.as_ref().unwrap();
            format!(
                "# -- Installing JS dependencies --\n\
                 ENV NODE_VERSION={node_version}\n\
                 RUN cd {wd} && {install_cmd_node} && tsx /api/langgraph_api/js/build.mts\n\
                 # -- End of JS dependencies install --"
            )
        } else {
            String::new()
        };

    let image_str = docker_tag(config, Some(base_image), api_version);

    // Build Dockerfile
    let mut docker_file_contents = Vec::new();

    // Syntax directive for additional contexts
    if !local_deps.additional_contexts.is_empty() {
        docker_file_contents.push("# syntax=docker/dockerfile:1.4".to_string());
        docker_file_contents.push(String::new());
    }

    let dep_vname = if escape_variables { "$$dep" } else { "$dep" };

    docker_file_contents.extend_from_slice(&[
        format!("FROM {image_str}"),
        String::new(),
        config.dockerfile_lines.join("\n"),
        String::new(),
        installs,
        String::new(),
        "# -- Installing all local dependencies --".to_string(),
        format!(
            "RUN for dep in /deps/*; do \\\n\
             {0}    echo \"Installing {dep_vname}\"; \\\n\
             {0}    if [ -d \"{dep_vname}\" ]; then \\\n\
             {0}        echo \"Installing {dep_vname}\"; \\\n\
             {0}        (cd \"{dep_vname}\" && {global_reqs_pip_install} -e .); \\\n\
             {0}    fi; \\\n\
             {0}done",
            "    "
        ),
        "# -- End of local dependencies install --".to_string(),
        env_vars.join("\n"),
        String::new(),
        js_inst_str,
        String::new(),
        get_pip_cleanup_lines(install_cmd, &build_tools_to_uninstall, &pip_installer),
        String::new(),
        local_deps
            .working_dir
            .as_ref()
            .map(|wd| format!("WORKDIR {wd}"))
            .unwrap_or_default(),
    ]);

    // Build additional contexts map
    let mut additional_contexts = IndexMap::new();
    for p in &local_deps.additional_contexts {
        if let Some((_, name)) = local_deps.real_pkgs.get(p) {
            additional_contexts.insert(name.clone(), p.to_string_lossy().to_string());
        } else if local_deps.faux_pkgs.contains_key(p) {
            let name = format!("outer-{}", p.file_name().unwrap().to_string_lossy());
            additional_contexts.insert(name, p.to_string_lossy().to_string());
        }
    }

    Ok((docker_file_contents.join("\n"), additional_contexts))
}

/// Generate a Dockerfile for a Node.js configuration.
pub fn node_config_to_docker(
    config_path: &Path,
    config: &mut Config,
    base_image: &str,
    api_version: Option<&str>,
    install_command: Option<&str>,
    build_command: Option<&str>,
    build_context: Option<&str>,
) -> Result<(String, IndexMap<String, String>), String> {
    let (faux_path, container_root) = if let Some(bc) = build_context {
        let relative_workdir = calculate_relative_workdir(config_path, bc)?;
        let container_name = Path::new(bc)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let fp = if relative_workdir.is_empty() {
            format!("/deps/{container_name}")
        } else {
            format!("/deps/{container_name}/{relative_workdir}")
        };
        (fp, Some(format!("/deps/{container_name}")))
    } else {
        let config_dir_name = config_path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        (format!("/deps/{config_dir_name}"), None)
    };

    let install_cmd = install_command
        .map(|s| s.to_string())
        .unwrap_or_else(|| get_node_pm_install_cmd(config_path, config));

    let image_str = docker_tag(config, Some(base_image), api_version);

    // Environment variables
    let env_vars = build_config_env_vars(config);

    let (install_step, build_step) = if let Some(_bc) = build_context {
        let cr = container_root.as_ref().unwrap();
        let inst = format!("RUN cd {cr} && {install_cmd}");
        let build = if let Some(bc) = build_command {
            format!("RUN cd {faux_path} && {bc}")
        } else {
            "RUN (test ! -f /api/langgraph_api/js/build.mts && echo \"Prebuild script not found, skipping\") || tsx /api/langgraph_api/js/build.mts".to_string()
        };
        (inst, build)
    } else {
        let inst = format!("RUN cd {faux_path} && {install_cmd}");
        let build = "RUN (test ! -f /api/langgraph_api/js/build.mts && echo \"Prebuild script not found, skipping\") || tsx /api/langgraph_api/js/build.mts".to_string();
        (inst, build)
    };

    let add_target = if build_context.is_some() {
        container_root.as_ref().unwrap().clone()
    } else {
        faux_path.clone()
    };

    let docker_file_contents = vec![
        format!("FROM {image_str}"),
        String::new(),
        config.dockerfile_lines.join("\n"),
        String::new(),
        format!("ADD . {add_target}"),
        String::new(),
        install_step,
        String::new(),
        env_vars.join("\n"),
        String::new(),
        format!("WORKDIR {faux_path}"),
        String::new(),
        build_step,
    ];

    Ok((docker_file_contents.join("\n"), IndexMap::new()))
}

/// Main entry point for Dockerfile generation.
#[allow(clippy::too_many_arguments)]
pub fn config_to_docker(
    config_path: &Path,
    config: &mut Config,
    base_image: Option<&str>,
    api_version: Option<&str>,
    install_command: Option<&str>,
    build_command: Option<&str>,
    build_context: Option<&str>,
    escape_variables: bool,
) -> Result<(String, IndexMap<String, String>), String> {
    let base_image = base_image
        .map(|s| s.to_string())
        .unwrap_or_else(|| super::docker_tag::default_base_image(config));

    if config.node_version.is_some() && config.python_version.is_none() {
        node_config_to_docker(
            config_path,
            config,
            &base_image,
            api_version,
            install_command,
            build_command,
            build_context,
        )
    } else {
        python_config_to_docker(config_path, config, &base_image, api_version, escape_variables)
    }
}

fn calculate_relative_workdir(config_path: &Path, build_context: &str) -> Result<String, String> {
    let config_dir = config_path.parent().unwrap().canonicalize().map_err(|e| {
        format!(
            "Could not resolve config directory: {e}"
        )
    })?;
    let build_context_path = Path::new(build_context).canonicalize().map_err(|e| {
        format!("Could not resolve build context: {e}")
    })?;

    config_dir
        .strip_prefix(&build_context_path)
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            if s == "." { String::new() } else { s }
        })
        .map_err(|_| {
            format!(
                "Configuration file {} is not under the build context {}. \
                 Please run the command from a directory that contains your langgraph.json file.",
                config_path.display(),
                build_context
            )
        })
}

// Helper functions to build Dockerfile sections

fn build_pip_reqs_str(local_deps: &LocalDeps, config_path: &Path, pip_install: &str) -> String {
    if local_deps.pip_reqs.is_empty() {
        return String::new();
    }

    let config_parent = config_path.parent().unwrap();
    let mut lines = Vec::new();

    for (reqpath, destpath) in &local_deps.pip_reqs {
        if local_deps
            .additional_contexts
            .iter()
            .any(|ac| reqpath.starts_with(ac))
        {
            let name = reqpath.parent().unwrap().file_name().unwrap().to_string_lossy();
            lines.push(format!(
                "COPY --from=outer-{name} requirements.txt {destpath}"
            ));
        } else if let Ok(rel) = reqpath.strip_prefix(config_parent) {
            lines.push(format!(
                "ADD {} {destpath}",
                rel.to_string_lossy().replace('\\', "/")
            ));
        }
    }

    let req_args: Vec<String> = local_deps
        .pip_reqs
        .iter()
        .map(|(_, dest)| format!("-r {dest}"))
        .collect();
    lines.push(format!("RUN {pip_install} {}", req_args.join(" ")));

    format!(
        "# -- Installing local requirements --\n{}\n# -- End of local requirements install --",
        lines.join("\n")
    )
}

fn build_faux_pkgs_str(local_deps: &LocalDeps, config_path: &Path) -> String {
    if local_deps.faux_pkgs.is_empty() {
        return String::new();
    }

    let _config_parent = config_path.parent().unwrap();
    let mut sections = Vec::new();

    for (fullpath, (relpath, destpath)) in &local_deps.faux_pkgs {
        let name = fullpath.file_name().unwrap().to_string_lossy();

        let add_line = if local_deps.additional_contexts.contains(fullpath) {
            format!(
                "# -- Adding non-package dependency {name} --\n\
                 COPY --from=outer-{name} . {destpath}"
            )
        } else {
            format!(
                "# -- Adding non-package dependency {name} --\n\
                 ADD {relpath} {destpath}"
            )
        };

        let section = format!(
            "{add_line}\n\
             RUN set -ex && \\\\\n\
             {s}for line in '[project]' \\\\\n\
             {s}            'name = \"{name}\"' \\\\\n\
             {s}            'version = \"0.1\"' \\\\\n\
             {s}            '[tool.setuptools.package-data]' \\\\\n\
             {s}            '\"*\" = [\"**/*\"]' \\\\\n\
             {s}            '[build-system]' \\\\\n\
             {s}            'requires = [\"setuptools>=61\"]' \\\\\n\
             {s}            'build-backend = \"setuptools.build_meta\"'; do \\\\\n\
             {s}    echo \"$line\" >> /deps/outer-{name}/pyproject.toml; \\\\\n\
             {s}done\n\
             # -- End of non-package dependency {name} --",
            s = "    "
        );
        sections.push(section);
    }

    sections.join("\n\n")
}

fn build_local_pkgs_str(local_deps: &LocalDeps, config_path: &Path) -> String {
    if local_deps.real_pkgs.is_empty() {
        return String::new();
    }

    let _config_parent = config_path.parent().unwrap();
    let mut lines = Vec::new();

    for (fullpath, (relpath, name)) in &local_deps.real_pkgs {
        if local_deps.additional_contexts.contains(fullpath) {
            lines.push(format!(
                "# -- Adding local package {relpath} --\n\
                 COPY --from={name} . /deps/{name}\n\
                 # -- End of local package {relpath} --"
            ));
        } else {
            lines.push(format!(
                "# -- Adding local package {relpath} --\n\
                 ADD {relpath} /deps/{name}\n\
                 # -- End of local package {relpath} --"
            ));
        }
    }

    lines.join("\n")
}

/// Generate compose YAML fragment for config (used by the up command).
pub fn config_to_compose(
    config_path: &Path,
    config: &mut Config,
    base_image: Option<&str>,
    api_version: Option<&str>,
    image: Option<&str>,
    watch: bool,
) -> Result<String, String> {
    let base_image = base_image
        .map(|s| s.to_string())
        .unwrap_or_else(|| super::docker_tag::default_base_image(config));

    let env_vars_str = match &config.env {
        Some(EnvConfig::Dict(d)) => d
            .iter()
            .map(|(k, v)| format!("            {k}: \"{v}\""))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };

    let env_file_str = match &config.env {
        Some(EnvConfig::File(f)) => format!("env_file: {f}"),
        _ => String::new(),
    };

    let watch_str = if watch {
        let dependencies = if config.dependencies.is_empty() {
            vec![".".to_string()]
        } else {
            config.dependencies.clone()
        };
        let config_name = config_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut watch_paths = vec![config_name];
        for dep in &dependencies {
            if dep.starts_with('.') {
                watch_paths.push(dep.clone());
            }
        }
        let watch_actions: Vec<String> = watch_paths
            .iter()
            .map(|p| format!("                - path: {p}\n                  action: rebuild"))
            .collect();
        format!(
            "\n        develop:\n            watch:\n{}\n",
            watch_actions.join("\n")
        )
    } else {
        String::new()
    };

    if image.is_some() {
        return Ok(format!(
            "\n{env_vars_str}\n        {env_file_str}\n        {watch_str}\n"
        ));
    }

    let (dockerfile, additional_contexts) = config_to_docker(
        config_path,
        config,
        Some(&base_image),
        api_version,
        None,
        None,
        None,
        true, // escape_variables
    )?;

    let additional_contexts_str = if !additional_contexts.is_empty() {
        let lines: Vec<String> = additional_contexts
            .iter()
            .map(|(name, path)| format!("                - {name}: {path}"))
            .collect();
        format!(
            "\n            additional_contexts:\n{}",
            lines.join("\n")
        )
    } else {
        String::new()
    };

    // Indent dockerfile for inline use
    let indented_dockerfile = dockerfile
        .lines()
        .map(|line| format!("                {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "\n{env_vars_str}\n        {env_file_str}\n        pull_policy: build\n        build:\n            context: .{additional_contexts_str}\n            dockerfile_inline: |\n{indented_dockerfile}\n        {watch_str}\n"
    ))
}
