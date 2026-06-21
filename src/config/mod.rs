use crate::app_name::AppName;
use crate::read_file;
use crate::templating::{
    CADDYFILE_REVERSE_PROXY, CADDYFILE_STATIC_FILES, SYSTEMD_WEBAPP_SERVICE, TemplateAndUserVars,
};
use crate::util::RelativePathResolver;
use miette::{IntoDiagnostic, NamedSource, Report, SourceCode, WrapErr, miette};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toml_span::value::ValueInner;
use toml_span::{DeserError, Deserialize, Spanned};

mod deserialize;
mod validate;

static DEFAULT_SYSTEMD_UNIT_EXTENSION: &str = "service";

pub fn load(
    app_name: &AppName,
    path: &Path,
    uses_default_config_path: bool,
) -> Result<AppConfig, Report> {
    let config_toml = fs::read_to_string(path).into_diagnostic().with_context(|| {
        let mut msg = format!("failed to load configuration file at `{}`", path.display());
        if uses_default_config_path {
            msg.push_str("\n\nno custom path was provided, so the default path was used... did you forget to specify a custom path?");
        }

        msg
    })?;

    load_from_str(app_name.as_str(), &config_toml, Some(path))
}

fn load_from_str(
    app_name: &str,
    config_toml: &str,
    config_path: Option<&Path>,
) -> Result<AppConfig, Report> {
    let miette_source: Arc<dyn SourceCode + 'static> = if let Some(path) = config_path
        && let Ok(path) = path.canonicalize()
    {
        let path = path.display().to_string();
        Arc::new(NamedSource::new(path, config_toml.to_string()))
    } else {
        Arc::new(config_toml.to_string())
    };

    let mut value = toml_span::parse(config_toml).map_err(|e| {
        deserialize::toml_error_to_report(DeserError::from(e), miette_source.clone())
    })?;
    let config = AppConfig::deserialize(&mut value)
        .map_err(|e| deserialize::toml_error_to_report(e, miette_source.clone()))?;
    validate::validate(app_name, &config).map_err(|errors| errors.into_report(miette_source))?;
    Ok(config)
}

pub struct AppConfig {
    /// Configuration related to the project's artifacts
    artifacts: Option<Spanned<ArtifactsConfig>>,
    /// Configuration related to the network
    network: Option<Spanned<NetworkConfig>>,
    /// Configuration related to the project's Caddyfile (if any)
    caddyfile: Option<Spanned<CaddyfileConfig>>,
    /// Configuration related to the project's systemd units (if any)
    systemd_units: Vec<Spanned<SystemdUnitConfig>>,
}

impl AppConfig {
    pub fn artifacts(&self) -> Option<&ArtifactsConfig> {
        self.artifacts.as_ref().map(|a| &a.value)
    }

    pub fn caddyfile(&self) -> Option<&CaddyfileConfig> {
        self.caddyfile.as_ref().map(|c| &c.value)
    }

    pub fn systemd_units(&self) -> impl ExactSizeIterator<Item = &SystemdUnitConfig> {
        self.systemd_units.iter().map(|u| &u.value)
    }

    pub fn reserved_ports(&self) -> u32 {
        self.network
            .as_ref()
            .and_then(|n| n.value.reserve_ports.as_ref())
            .map(|p| p.value)
            .unwrap_or_default()
    }
}

pub struct ArtifactsConfig {
    /// The relative path to the directory where the to-be-deployed artifacts are located
    ///
    /// Note: if the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    path: Spanned<PathBuf>,
}

impl ArtifactsConfig {
    pub fn path(&self) -> &Path {
        &self.path.value
    }
}

#[derive(Default)]
pub struct NetworkConfig {
    /// The number of ports that vade should reserve for this application
    reserve_ports: Option<Spanned<u32>>,
}

pub struct SystemdUnitConfig {
    /// Whether the unit should be automatically enabled after being deployed
    ///
    /// Defaults to `true`
    #[allow(dead_code)]
    enable: Option<Spanned<bool>>,
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
    filename_suffix: Option<Spanned<String>>,
    /// The file extension of this systemd unit
    ///
    /// Defaults to `service`
    file_extension: Option<Spanned<String>>,
    /// The template from which this systemd unit file will be rendered
    template: Spanned<TemplateConfig>,
}

impl SystemdUnitConfig {
    pub fn filename(&self, app_name: &str) -> String {
        let suffix = self
            .filename_suffix
            .as_ref()
            .map(|s| format!("-{}", s.value))
            .unwrap_or_default();

        let extension = self
            .file_extension
            .as_ref()
            .map(|f| f.value.as_str())
            .unwrap_or(DEFAULT_SYSTEMD_UNIT_EXTENSION);

        format!("{app_name}{suffix}.{extension}")
    }

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
                .value
                .load_template(path_resolver, "systemd unit", Self::get_builtin)?;

        Ok(TemplateAndUserVars {
            template,
            user_vars: self.template.value.load_user_vars(),
        })
    }
}

pub struct CaddyfileConfig {
    /// The template from which the application's Caddyfile will be rendered
    template: Spanned<TemplateConfig>,
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
                .value
                .load_template(path_resolver, "Caddyfile", Self::get_builtin)?;

        Ok(TemplateAndUserVars {
            template,
            user_vars: self.template.value.load_user_vars(),
        })
    }
}

pub struct TemplateConfig {
    /// The source from which the template will be loaded
    ///
    /// The following source types are supported:
    /// - `builtin`: loads one of the built-in templates (those under `src/resources/systemd-unit-templates`)
    /// - `inline`: loads the template from the provided string
    /// - `file`: loads the template from the filesystem. If the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    ///
    /// Templates are rendered using `minijinja`
    source: Spanned<TemplateSource>,
    /// Variables to use when rendering the template
    ///
    /// These variables are placed under the `vars` object, so e.g., a variable called `domains`
    /// will be available at `vars.domains`
    vars: Spanned<HashMap<String, minijinja::Value>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateSource {
    Builtin(String),
    File(PathBuf),
    Inline(String),
}

impl TemplateConfig {
    fn load_template(
        &self,
        path_resolver: &RelativePathResolver,
        kind: &str,
        get_builtin: fn(&str) -> Option<&'static str>,
    ) -> Result<String, Report> {
        match &self.source.value {
            TemplateSource::Builtin(template_name) => {
                let builtin = get_builtin(template_name)
                    .ok_or(miette!("unknown built-in {kind} template: {template_name}"))?;
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
        self.vars.value.clone()
    }
}

fn value_to_minijinja(value: ValueInner) -> minijinja::Value {
    match value {
        ValueInner::String(s) => s.into_owned().into(),
        ValueInner::Integer(x) => x.into(),
        ValueInner::Float(x) => x.into(),
        ValueInner::Boolean(x) => x.into(),
        ValueInner::Array(array) => {
            minijinja::Value::from_iter(array.into_iter().map(|mut v| value_to_minijinja(v.take())))
        }
        ValueInner::Table(table) => {
            let mut obj = HashMap::new();
            for (k, mut v) in table {
                obj.insert(k.name.into_owned(), value_to_minijinja(v.take()));
            }

            minijinja::Value::from_object(obj)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::assert_matches;

    fn test_load_from_str(src: &str) -> Result<AppConfig, Report> {
        load_from_str("my-app", src, None)
    }

    #[test]
    fn test_load_minimal() {
        let config = test_load_from_str("").unwrap();
        assert!(config.systemd_units.is_empty());
        assert!(config.caddyfile.is_none());
        assert!(config.artifacts.is_none());
    }

    #[test]
    fn test_load_unknown_key_top_level() {
        let config = test_load_from_str("asdf = 42");
        assert!(config.is_err());
    }

    #[test]
    fn test_load_unknown_key_systemd_unit() {
        let src = r#"
[[systemd-unit]]
[systemd-unit.template]
builtin = "webapp.service"
asdf = 42
"#;

        let config = test_load_from_str(src);
        assert!(config.is_err());
    }

    #[test]
    fn test_load_reports_all_problems_with_spans() {
        let src = r#"
[network]
reserve-ports = "not-a-number"

[[systemd-unit]]
[systemd-unit.template]
builtin = "webapp.service"
inline = "oops, two sources"
typo-key = 1
"#;

        let Err(err) = test_load_from_str(src) else {
            panic!("expected the configuration to fail to load");
        };

        // The source is attached, so miette can render the spans against it
        assert!(err.source_code().is_some());

        let labels: Vec<String> = err
            .labels()
            .into_iter()
            .flatten()
            .filter_map(|l| l.label().map(str::to_string))
            .collect();

        assert!(labels.iter().any(|l| l == "expected u32, found string"));
        // Both conflicting sources are flagged
        assert_eq!(
            labels
                .iter()
                .filter(|l| l.contains("conflicting template source"))
                .count(),
            2
        );
        assert!(labels.iter().any(|l| l == "unexpected key `typo-key`"));
    }

    #[test]
    fn test_load_rejects_duplicate_unit_filenames() {
        // Both units default to no suffix and the `service` extension, so they would write to the
        // same file on the server
        let src = r#"
[[systemd-unit]]
[systemd-unit.template]
inline = "first"

[[systemd-unit]]
[systemd-unit.template]
inline = "second"
"#;

        let Err(err) = test_load_from_str(src) else {
            panic!("expected the configuration to fail to load");
        };

        let labels: Vec<String> = err
            .labels()
            .into_iter()
            .flatten()
            .filter_map(|l| l.label().map(str::to_string))
            .collect();

        // Both conflicting units are flagged
        assert_eq!(
            labels
                .iter()
                .filter(|l| l.contains("`my-app.service` is declared multiple times"))
                .count(),
            2
        );
    }

    #[test]
    fn test_load_allows_units_differing_by_suffix() {
        // Same extension, but different suffixes, so the filenames don't collide
        let src = r#"
[[systemd-unit]]
filename-suffix = "a"
[systemd-unit.template]
inline = "first"

[[systemd-unit]]
filename-suffix = "b"
[systemd-unit.template]
inline = "second"
"#;

        let config = test_load_from_str(src).unwrap();
        assert_eq!(config.systemd_units().len(), 2);
    }

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

        let config = test_load_from_str(src).unwrap();
        assert_eq!(
            config.artifacts().unwrap().path().to_string_lossy(),
            "artifacts"
        );
        assert_eq!(config.reserved_ports(), 1);
        assert_eq!(config.systemd_units().len(), 1);
        assert!(config.caddyfile().is_some());

        let systemd_config = config.systemd_units().next().unwrap();
        assert_eq!(systemd_config.filename("my-app"), "my-app.service");
        assert_eq!(
            systemd_config.template.value.source.value,
            TemplateSource::Builtin("webapp.service".to_string())
        );
        assert_eq!(systemd_config.template.value.vars.value.len(), 1);
        assert_eq!(
            systemd_config.template.value.vars.value["exec_start"],
            minijinja::Value::from(
                "{{ vade.app_paths.active_artifacts_dir }}/goatcounter serve -listen :{{ vade.app_port }}"
            )
        );

        let caddyfile_config = config.caddyfile().unwrap();
        assert_eq!(
            caddyfile_config.template.value.source.value,
            TemplateSource::Builtin("reverse-proxy".to_string())
        );
        assert_eq!(caddyfile_config.template.value.vars.value.len(), 1);
        assert_eq!(
            caddyfile_config.template.value.vars.value["domains"],
            minijinja::Value::from(vec!["goats.example.com"])
        );
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

        let config = test_load_from_str(src).unwrap();
        assert!(config.artifacts().is_none());
        assert_eq!(config.reserved_ports(), 0);
        assert_eq!(config.systemd_units().len(), 2);
        assert!(config.caddyfile().is_none());

        let systemd_config = config.systemd_units().next().unwrap();
        assert!(systemd_config.enable.as_ref().is_some_and(|x| !x.value));
        assert_eq!(systemd_config.filename("my-app"), "my-app.service");
        assert_matches!(
            systemd_config.template.value.source.value,
            TemplateSource::Inline(_)
        );
        assert_eq!(systemd_config.template.value.vars.value.len(), 0);

        let systemd_config = config.systemd_units().nth(1).unwrap();
        assert!(systemd_config.enable.is_none());
        assert_eq!(systemd_config.filename("my-app"), "my-app.timer");
        assert_matches!(
            systemd_config.template.value.source.value,
            TemplateSource::Inline(_)
        );
        assert_eq!(systemd_config.template.value.vars.value.len(), 0);
    }
}
