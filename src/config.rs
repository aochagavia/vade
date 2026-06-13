use crate::templating::{APP_PORT_VAR, APP_PORTS_VAR};
use crate::templating::{
    CADDYFILE_REVERSE_PROXY, CADDYFILE_STATIC_FILES, SYSTEMD_APPLICATION, TemplateAndExtraVars,
};
use crate::{read_file, resolve_relative_to};
use rootcause::prelude::ResultExt;
use rootcause::{Report, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn load(path: &Path, uses_default_config_path: bool) -> Result<ApplicationConfig, Report> {
    let config_toml = fs::read(path).context_with(|| {
        let mut msg = format!("failed to load configuration file at `{}`", path.display());
        if uses_default_config_path {
            msg.push_str("\n\nno custom path was provided, so the default path was used... did you forget to specify a custom path?");
        }

        msg
    })?;

    let config = toml::from_slice(&config_toml).context("invalid application configuration")?;
    Ok(config)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct ApplicationConfig {
    /// Configuration related to the project's artifacts
    pub artifacts: ArtifactsConfig,
    /// Configuration related to the network
    #[serde(default)]
    pub network: NetworkConfig,
    /// Configuration related to the project's Caddyfile (if any)
    pub caddyfile: Option<CaddyfileConfig>,
    /// Configuration related to the project's systemd unit (if any)
    pub systemd_unit: Option<SystemdUnitConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct ArtifactsConfig {
    /// The relative path to the directory where the to-be-deployed artifacts are located
    ///
    /// Note: if the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct NetworkConfig {
    /// The number of ports that vade should reserve for this application
    #[serde(default)]
    pub reserve_ports: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SystemdUnitConfig {
    /// The path to the systemd unit file template that this application should use for deployment
    /// (if any)
    ///
    /// The following path types are supported:
    /// - `builtin://<name>`: loads one of the built-in templates (those under `src/resources/systemd-unit-templates`)
    /// - `<path>`: loads the template from the filesystem. If the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    ///
    /// Templates are rendered using `minijinja`
    pub template: String,
    /// The command that should be passed to the unit's `ExecStart=`
    pub exec_start: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CaddyfileConfig {
    /// The path to the Caddyfile template that this application should use for deployment
    /// (if any)
    ///
    /// The following path types are supported:
    /// - `builtin://<name>`: loads one of the built-in templates (those under `src/resources/caddyfile-templates`)
    /// - `<path>`: loads the template from the filesystem. If the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    ///
    /// Templates are rendered using `minijinja`
    template: String,
    /// Domains that Caddy should route to our application
    #[serde(default)]
    domains: Vec<String>,
}

impl SystemdUnitConfig {
    pub fn load_template(
        &self,
        config_parent_path: &Path,
        network_config: &NetworkConfig,
    ) -> Result<TemplateAndExtraVars, Report> {
        let template = if let Some(template_name) = self.template.strip_prefix("builtin://") {
            match template_name {
                "application" => SYSTEMD_APPLICATION.to_string(),
                _ => bail!("unknown built-in systemd unit template: {template_name}"),
            }
        } else {
            let systemd_unit_path =
                resolve_relative_to(config_parent_path, Path::new(&self.template));
            read_file(&systemd_unit_path)?
        };

        let mut extra_template_vars = HashMap::new();
        extra_template_vars.insert("SYSTEMD_UNIT_EXEC_START", self.exec_start.clone().into());

        let mut extra_environment_entries = Vec::new();
        if network_config.reserve_ports > 0 {
            extra_environment_entries.push(format!("PORT={{{{ {APP_PORT_VAR} }}}}"));
            extra_environment_entries
                .push(format!(r#"PORTS={{{{ {APP_PORTS_VAR} | join(",") }}}}"#));
        }
        extra_template_vars.insert("extra_environments", extra_environment_entries.into());

        inject_port_variables(&mut extra_template_vars, network_config);

        Ok(TemplateAndExtraVars {
            template,
            extra_vars: extra_template_vars,
        })
    }
}

impl CaddyfileConfig {
    pub fn load_template(
        &self,
        config_parent_path: &Path,
        network_config: &NetworkConfig,
    ) -> Result<TemplateAndExtraVars, Report> {
        let template = if let Some(template_name) = self.template.strip_prefix("builtin://") {
            match template_name {
                "static-files" => CADDYFILE_STATIC_FILES.to_string(),
                "reverse-proxy" => CADDYFILE_REVERSE_PROXY.to_string(),
                _ => bail!("unknown built-in Caddyfile template: {template_name}"),
            }
        } else {
            let template_path = resolve_relative_to(config_parent_path, Path::new(&self.template));
            read_file(&template_path)?
        };

        let mut extra_vars = HashMap::new();
        extra_vars.insert("CADDYFILE_DOMAINS", self.domains.clone().into());
        inject_port_variables(&mut extra_vars, network_config);

        Ok(TemplateAndExtraVars {
            template,
            extra_vars,
        })
    }
}

fn inject_port_variables(
    extra_vars: &mut HashMap<&'static str, minijinja::Value>,
    network_config: &NetworkConfig,
) {
    if network_config.reserve_ports > 0 {
        // This variable resolves to itself, so the rendered file still has an `APP_PORT`
        // variable in it that can be replaced at deploy time on the server (we usually don't know
        // the port number before that moment).
        extra_vars.insert(APP_PORT_VAR, format!("{{{{ {APP_PORT_VAR} }}}}").into());
    }

    // In case the application reserves more than one port, template writers can use
    // `{{ APP_PORTS[i] }}` to refer to each one. Similar to `APP_PORT`, the variable will resolve
    // to itself and will be replaced at deploy time.
    let mut values = Vec::with_capacity(network_config.reserve_ports as usize);
    for i in 0..network_config.reserve_ports {
        values.push(format!("{{{{ {APP_PORTS_VAR}[{i}] }}}}"));
    }
    extra_vars.insert(APP_PORTS_VAR, values.into());
}
