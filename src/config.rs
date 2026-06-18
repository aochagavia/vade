use crate::read_file;
use crate::templating::{
    CADDYFILE_REVERSE_PROXY, CADDYFILE_STATIC_FILES, SYSTEMD_WEBAPP_SERVICE, TemplateAndUserVars,
};
use crate::util::RelativePathResolver;
use rootcause::prelude::ResultExt;
use rootcause::{Report, report};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub fn load(path: &Path, uses_default_config_path: bool) -> Result<AppConfig, Report> {
    let config_toml = fs::read_to_string(path).context_with(|| {
        let mut msg = format!("failed to load configuration file at `{}`", path.display());
        if uses_default_config_path {
            msg.push_str("\n\nno custom path was provided, so the default path was used... did you forget to specify a custom path?");
        }

        msg
    })?;

    load_from_slice(&config_toml)
}

fn load_from_slice(config_toml: &str) -> Result<AppConfig, Report> {
    let config = toml::from_str(config_toml).context("invalid application configuration")?;
    Ok(config)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Configuration related to the project's artifacts
    pub artifacts: Option<ArtifactsConfig>,
    /// Configuration related to the network
    #[serde(default)]
    pub network: NetworkConfig,
    /// Configuration related to the project's Caddyfile (if any)
    pub caddyfile: Option<CaddyfileConfig>,
    /// Configuration related to the project's systemd units (if any)
    #[serde(rename = "systemd-unit")]
    #[serde(default)]
    pub systemd_units: Vec<SystemdUnitConfig>,
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
pub struct TemplateConfig {
    /// The source from which the template will be loaded
    ///
    /// The following source types are supported:
    /// - `builtin`: loads one of the built-in templates (those under `src/resources/systemd-unit-templates`)
    /// - `inline`: loads the template from the provided string
    /// - `file`: loads the template from the filesystem. If the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    ///
    /// Templates are rendered using `minijinja`
    #[serde(flatten)]
    pub source: TemplateSource,
    /// Variables to use when rendering the template
    ///
    /// These variables are placed under the `vars` object, so e.g., a variable called `domains`
    /// will be available at `vars.domains`
    #[serde(default)]
    pub vars: HashMap<String, toml::Value>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[repr(u8)]
pub enum TemplateSource {
    Builtin(String),
    File(PathBuf),
    Inline(String),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SystemdUnitConfig {
    /// Whether the unit should be automatically enabled after being deployed
    ///
    /// Defaults to `true`
    #[serde(default = "default_unit_enable")]
    pub enable: bool,
    /// The filename suffix of this systemd unit (if any)
    ///
    /// On the server, systemd unit file names need to be unique. To prevent collisions, unit names
    /// are namespaced based on the app name. If you need to differentiate between multiple
    /// units in a single project, you can do so by assigning them different suffixes.
    ///
    /// The following examples show the unit file names, as they would be in the sever, for an
    /// app called `foo` (assuming the default `service` file extension):
    ///
    /// - No suffix: `foo.service`
    /// - Suffix set to `bar`: `foo-bar.service`
    pub filename_suffix: Option<String>,
    /// The file extension of this systemd unit
    ///
    /// Defaults to `service`
    #[serde(default = "default_unit_file_extension")]
    pub file_extension: String,
    /// The template from which this systemd unit file will be rendered
    pub template: TemplateConfig,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CaddyfileConfig {
    /// The template from which the application's Caddyfile will be rendered
    template: TemplateConfig,
}

impl TemplateConfig {
    fn load_template(
        &self,
        path_resolver: &RelativePathResolver,
        kind: &str,
        get_builtin: fn(&str) -> Option<&'static str>,
    ) -> Result<String, Report> {
        match &self.source {
            TemplateSource::Builtin(template_name) => {
                let builtin = get_builtin(template_name)
                    .ok_or(report!("unknown built-in {kind} template: {template_name}"))?;
                Ok(builtin.to_string())
            }
            TemplateSource::File(path) => {
                let systemd_unit_path = path_resolver.resolve(path);
                Ok(read_file(&systemd_unit_path)?)
            }
            TemplateSource::Inline(s) => Ok(s.clone()),
        }
    }

    fn load_user_vars(&self) -> HashMap<String, minijinja::Value> {
        let mut obj = HashMap::new();
        for (k, v) in &self.vars {
            obj.insert(k.clone(), toml_to_minijinja(v));
        }

        obj
    }
}

impl SystemdUnitConfig {
    fn get_builtin(template_name: &str) -> Option<&'static str> {
        match template_name {
            "webapp.service" => Some(SYSTEMD_WEBAPP_SERVICE),
            _ => None,
        }
    }

    pub fn load_template(
        &self,
        path_resolver: &RelativePathResolver,
    ) -> Result<TemplateAndUserVars, Report> {
        let template =
            self.template
                .load_template(path_resolver, "systemd unit", Self::get_builtin)?;

        Ok(TemplateAndUserVars {
            template,
            user_vars: self.template.load_user_vars(),
        })
    }
}

impl CaddyfileConfig {
    fn get_builtin(template_name: &str) -> Option<&'static str> {
        match template_name {
            "static-files" => Some(CADDYFILE_STATIC_FILES),
            "reverse-proxy" => Some(CADDYFILE_REVERSE_PROXY),
            _ => None,
        }
    }

    pub fn load_template(
        &self,
        path_resolver: &RelativePathResolver,
    ) -> Result<TemplateAndUserVars, Report> {
        let template =
            self.template
                .load_template(path_resolver, "Caddyfile", Self::get_builtin)?;

        Ok(TemplateAndUserVars {
            template,
            user_vars: self.template.load_user_vars(),
        })
    }
}

fn toml_to_minijinja(value: &toml::Value) -> minijinja::Value {
    match value {
        toml::Value::String(x) => x.into(),
        &toml::Value::Integer(x) => x.into(),
        &toml::Value::Float(x) => x.into(),
        &toml::Value::Boolean(x) => x.into(),
        &toml::Value::Datetime(x) => x.to_string().into(),
        toml::Value::Array(array) => {
            let mut xs = Vec::with_capacity(array.len());
            for x in array {
                xs.push(toml_to_minijinja(x));
            }

            minijinja::Value::from_iter(xs)
        }
        toml::Value::Table(table) => {
            let mut obj = HashMap::new();
            for (k, v) in table {
                obj.insert(k.clone(), toml_to_minijinja(v));
            }

            minijinja::Value::from_object(obj)
        }
    }
}

fn default_unit_enable() -> bool {
    true
}

fn default_unit_file_extension() -> String {
    "service".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    #[test]
    fn test_load_single_unit() {
        let src = r#"
[artifacts]
path = "artifacts"

[network]
reserve-ports = 1

[[systemd-unit]]
[systemd-unit.template]
builtin = "webapp.service"
vars = {
  exec_start = "{{ vade.app_paths.active_artifacts_dir }}/goatcounter serve -listen :{{ vade.app_port }}"
}

[caddyfile.template]
builtin = "reverse-proxy"
vars = {
  domains = ["goats.example.com"]
}
"#;

        let config = load_from_slice(src).unwrap();
        assert_eq!(
            config.artifacts.unwrap().path.to_string_lossy(),
            "artifacts"
        );
        assert_eq!(config.network.reserve_ports, 1);
        assert_eq!(config.systemd_units.len(), 1);
        assert!(config.caddyfile.is_some());

        let systemd_config = &config.systemd_units[0];
        assert!(systemd_config.filename_suffix.is_none());
        assert_eq!(
            systemd_config.template.source,
            TemplateSource::Builtin("webapp.service".to_string())
        );
        assert_eq!(systemd_config.template.vars.len(), 1);
        assert_eq!(systemd_config.template.vars["exec_start"], toml::Value::String("{{ vade.app_paths.active_artifacts_dir }}/goatcounter serve -listen :{{ vade.app_port }}".to_string()));

        let caddyfile_config = config.caddyfile.unwrap();
        assert_eq!(
            caddyfile_config.template.source,
            TemplateSource::Builtin("reverse-proxy".to_string())
        );
        assert_eq!(caddyfile_config.template.vars.len(), 1);

        let expected_domains: toml::Value = vec!["goats.example.com"].into();
        assert_eq!(caddyfile_config.template.vars["domains"], expected_domains);
    }

    #[test]
    fn test_load_two_units() {
        let src = r#"
[[systemd-unit]]
enable = false

[systemd-unit.template]
inline = """
[Unit]
Description=Touches a file, demonstrating that the service ran to completion

[Service]
ExecStart=touch /tmp/a-new-file-is-born
"""

[[systemd-unit]]
file-extension = "timer"

[systemd-unit.template]
inline = """
[Timer]
# Fires right after the timer is activated (e.g. on boot or `systemctl start`)
OnActiveSec=0s
# Then repeat every hour relative to the last time the service was activated
OnUnitActiveSec=1h

[Install]
WantedBy=timers.target
"""
"#;

        let config = load_from_slice(src).unwrap();
        assert!(config.artifacts.is_none());
        assert_eq!(config.network.reserve_ports, 0);
        assert_eq!(config.systemd_units.len(), 2);
        assert!(config.caddyfile.is_none());

        let systemd_config = &config.systemd_units[0];
        assert!(!systemd_config.enable);
        assert!(systemd_config.filename_suffix.is_none());
        assert_matches!(systemd_config.template.source, TemplateSource::Inline(_));
        assert_eq!(systemd_config.template.vars.len(), 0);

        let systemd_config = &config.systemd_units[1];
        assert!(systemd_config.enable);
        assert!(systemd_config.filename_suffix.is_none());
        assert_matches!(systemd_config.template.source, TemplateSource::Inline(_));
        assert_eq!(systemd_config.template.vars.len(), 0);
    }
}
