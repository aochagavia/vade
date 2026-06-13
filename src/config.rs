use crate::templating::{APP_PORT_VAR, APP_PORTS_VAR};
use crate::templating::{
    CADDYFILE_REVERSE_PROXY, CADDYFILE_STATIC_FILES, SYSTEMD_APPLICATION, TemplateAndExtraVars,
};
use crate::{path_relative_to, read_file};
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
    /// Note: this path is relative to the configuration file, not to the current working directory
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
pub enum SystemdUnitConfig {
    Custom(CustomSystemdUnitConfig),
    Generated(GeneratedSystemdUnitConfig),
}

#[derive(Serialize, Deserialize)]
pub struct CustomSystemdUnitConfig {
    /// The relative path to the systemd unit file that this application should use for deployment
    /// (if any)
    ///
    /// The file itself is templated with `minijinja`, so the relevant paths are injected by the
    /// CLI. That way, the default systemd unit can be reused without changes in most cases.
    ///
    /// Note: this path is relative to the configuration file, not to the current working directory
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GeneratedSystemdUnitConfig {
    /// The systemd unit template that we should render
    pub template: String,
    /// The command that should be passed to the unit's `ExecStart=`
    pub exec_start: String,
    /// Extra entries that should become part of the unit, each one in its own `Environment=` line
    #[serde(default)]
    pub extra_environment_entries: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaddyfileConfig {
    Custom(CustomCaddyfileConfig),
    Generated(GeneratedCaddyfileConfig),
}

#[derive(Serialize, Deserialize)]
pub struct CustomCaddyfileConfig {
    /// The relative path to the Caddyfile that this application should use for deployment (if any)
    ///
    /// Note: this path is relative to the configuration file, not to the current working directory
    path: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct GeneratedCaddyfileConfig {
    /// The Caddyfile template that we should render
    template: String,
    /// Domains that Caddy will route to our application
    domains: Vec<String>,
}

impl SystemdUnitConfig {
    pub fn load_template(
        &self,
        config_parent_path: &Path,
        network_config: &NetworkConfig,
    ) -> Result<TemplateAndExtraVars, Report> {
        let mut template = match self {
            SystemdUnitConfig::Custom(custom) => custom.load_template(config_parent_path)?,
            SystemdUnitConfig::Generated(generated) => generated.load_template(network_config)?,
        };

        inject_port_variables(&mut template.extra_vars, network_config);
        Ok(template)
    }
}

impl CustomSystemdUnitConfig {
    pub fn load_template(&self, config_parent_path: &Path) -> Result<TemplateAndExtraVars, Report> {
        let systemd_unit_path = path_relative_to(config_parent_path, self.path.clone());
        let template = read_file(&systemd_unit_path)?;
        Ok(TemplateAndExtraVars {
            template,
            extra_vars: Default::default(),
        })
    }
}

impl GeneratedSystemdUnitConfig {
    pub fn load_template(
        &self,
        network_config: &NetworkConfig,
    ) -> Result<TemplateAndExtraVars, Report> {
        let mut extra_template_vars = HashMap::new();
        extra_template_vars.insert("SYSTEMD_UNIT_EXEC_START", self.exec_start.clone().into());

        let mut extra_environment_entries = self.extra_environment_entries.clone();
        if network_config.reserve_ports > 0 {
            extra_environment_entries.push(format!("PORT={{{{ {APP_PORT_VAR} }}}}"));
            extra_environment_entries
                .push(format!(r#"PORTS={{{{ {APP_PORTS_VAR} | join(",") }}}}"#));
        }
        extra_template_vars.insert("extra_environments", extra_environment_entries.into());

        match self.template.as_str() {
            "application" => Ok(TemplateAndExtraVars {
                template: SYSTEMD_APPLICATION.into(),
                extra_vars: extra_template_vars,
            }),
            _ => bail!("invalid systemd unit template: {}", self.template),
        }
    }
}

impl CaddyfileConfig {
    pub fn load_template(
        &self,
        config_parent_path: &Path,
        network_config: &NetworkConfig,
    ) -> Result<TemplateAndExtraVars, Report> {
        let mut template = match self {
            CaddyfileConfig::Custom(custom) => custom.load_template(config_parent_path)?,
            CaddyfileConfig::Generated(generated) => generated.load_template()?,
        };

        inject_port_variables(&mut template.extra_vars, network_config);
        Ok(template)
    }
}

impl CustomCaddyfileConfig {
    pub fn load_template(&self, config_parent_path: &Path) -> Result<TemplateAndExtraVars, Report> {
        let caddyfile_path = path_relative_to(config_parent_path, self.path.clone());
        let template = read_file(&caddyfile_path)?;
        Ok(TemplateAndExtraVars {
            template,
            extra_vars: Default::default(),
        })
    }
}

impl GeneratedCaddyfileConfig {
    pub fn load_template(&self) -> Result<TemplateAndExtraVars, Report> {
        let mut extra_template_vars = HashMap::new();
        extra_template_vars.insert("CADDYFILE_DOMAINS", self.domains.clone().into());

        match self.template.as_str() {
            "static-files" => Ok(TemplateAndExtraVars {
                template: CADDYFILE_STATIC_FILES.into(),
                extra_vars: extra_template_vars,
            }),
            "reverse-proxy" => Ok(TemplateAndExtraVars {
                template: CADDYFILE_REVERSE_PROXY.into(),
                extra_vars: extra_template_vars,
            }),
            _ => bail!("invalid caddyfile template: {}", self.template),
        }
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
