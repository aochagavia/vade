use crate::app_name::AppName;
use crate::templating;
use crate::templating::{
    BuiltinTemplateKind, CADDYFILE_REVERSE_PROXY, CADDYFILE_STATIC_FILES, SYSTEMD_WEBAPP_SERVICE,
    TomlSource,
};
use crate::util::{RelativePathResolver, diagnostic, diagnostic_with_help};
use miette::{IntoDiagnostic, NamedSource, Report, SourceCode, WrapErr};
use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toml_span::value::ValueInner;
use toml_span::{DeserError, Deserialize, Span, Spanned};

mod deserialize;
mod validate;

static DEFAULT_SYSTEMD_UNIT_EXTENSION: &str = "service";

pub fn load(
    app_name: &AppName,
    path: &Path,
    uses_default_config_path: bool,
) -> Result<(AppConfig, TomlSource), Report> {
    let config_toml = fs::read_to_string(path).into_diagnostic().with_context(|| {
        let mut msg = format!("failed to load configuration file at `{}`", path.display());
        if uses_default_config_path {
            msg.push_str("\n\nno custom path was provided, so the default path was used... did you forget to specify a custom path?");
        }

        msg
    })?;

    let config = load_from_str(app_name.as_str(), &config_toml, Some(path))?;
    let config_src = TomlSource {
        path: path.display().to_string(),
        value: config_toml,
    };

    Ok((config, config_src))
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
}

pub struct ArtifactsConfig {
    /// The relative path to the directory where the to-be-deployed artifacts are located
    ///
    /// Note: if the path is relative, it will be resolved relative to the configuration file, not to the current working directory
    pub path: Spanned<PathBuf>,
}

impl ArtifactsConfig {
    pub fn span(&self) -> Range<usize> {
        self.path.span.start..self.path.span.end
    }
}

pub struct SystemdUnitConfig {
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
        toml_source: &TomlSource,
        path_resolver: &RelativePathResolver,
    ) -> Result<TemplateAndUserVars, Report> {
        load_template_inner(
            toml_source,
            &self.template.value,
            path_resolver,
            BuiltinTemplateKind::SystemdUnit,
            Self::get_builtin,
        )
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
        toml_source: &TomlSource,
        path_resolver: &RelativePathResolver,
    ) -> Result<TemplateAndUserVars, Report> {
        load_template_inner(
            toml_source,
            &self.template.value,
            path_resolver,
            BuiltinTemplateKind::Caddyfile,
            Self::get_builtin,
        )
    }
}

fn load_template_inner(
    toml_source: &TomlSource,
    template_config: &TemplateConfig,
    path_resolver: &RelativePathResolver,
    template_kind: BuiltinTemplateKind,
    get_builtin: fn(&str) -> Option<&'static str>,
) -> Result<TemplateAndUserVars, Report> {
    let template =
        template_config.load_template(toml_source, path_resolver, template_kind, get_builtin)?;

    let vars = template_config.vars.clone();

    Ok(TemplateAndUserVars {
        source: template,
        user_vars: UserVars::from_toml(vars.take()),
    })
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
    vars: Spanned<HashMap<String, UserVar>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TemplateSource {
    Builtin(String),
    File(PathBuf),
    Inline(Spanned<String>),
}

impl TemplateConfig {
    fn load_template(
        &self,
        toml_source: &TomlSource,
        path_resolver: &RelativePathResolver,
        kind: BuiltinTemplateKind,
        get_builtin: fn(&str) -> Option<&'static str>,
    ) -> Result<templating::TemplateSource, Report> {
        match &self.source.value {
            TemplateSource::Builtin(template_name) => {
                let builtin = get_builtin(template_name).ok_or_else(|| {
                    diagnostic(
                        "unknown builtin template",
                        format!("there is no {kind} template with this name"),
                        self.source.span,
                        toml_source.to_named_source(),
                    )
                })?;
                Ok(templating::TemplateSource::builtin(
                    template_name.clone(),
                    kind,
                    builtin.to_string(),
                ))
            }
            TemplateSource::File(path) => {
                let systemd_unit_path = path_resolver.resolve(path);
                let value = fs::read_to_string(&*systemd_unit_path).map_err(|e| {
                    diagnostic_with_help(
                        "failed to load template",
                        format!("reading the file resulted in an error: {e}"),
                        format!("the path resolved to `{}`", systemd_unit_path.display()),
                        self.source.span,
                        toml_source.to_named_source(),
                    )
                })?;
                Ok(templating::TemplateSource::file(
                    systemd_unit_path.display().to_string(),
                    value,
                ))
            }
            TemplateSource::Inline(s) => {
                Ok(templating::TemplateSource::inline(s.span, s.value.clone()))
            }
        }
    }
}

pub struct TemplateAndUserVars {
    pub source: templating::TemplateSource,
    pub user_vars: UserVars,
}

#[derive(Default)]
pub struct UserVars {
    vars: HashMap<String, UserVar>,
}

impl UserVars {
    fn from_toml(vars: HashMap<String, UserVar>) -> Self {
        Self { vars }
    }

    pub fn strings_mut(&mut self) -> Vec<&mut UserVarString> {
        let mut strings = Vec::new();
        for v in self.vars.values_mut() {
            strings.extend(v.strings_mut());
        }

        strings
    }

    pub fn set(&mut self, key: String, value: UserVar) {
        // note: only top-level set is supported for now
        self.vars.insert(key, value);
    }

    pub fn into_minijinja(self) -> minijinja::Value {
        self.vars
            .into_iter()
            .map(|(k, v)| (k, v.into_minijinja()))
            .collect()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UserVar {
    String(UserVarString),
    Scalar(minijinja::Value),
    Object(HashMap<String, UserVar>),
    List(Vec<UserVar>),
}

impl UserVar {
    pub fn from_json(path: &str, value: serde_json::Value) -> Self {
        use serde_json::Value;
        match value {
            Value::String(s) => UserVar::String(UserVarString::json(s, path.to_string())),
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                Self::Scalar(minijinja::Value::from_serialize(value))
            }
            Value::Array(array) => Self::List(
                array
                    .into_iter()
                    .map(|x| UserVar::from_json(path, x))
                    .collect(),
            ),
            Value::Object(object) => Self::Object(
                object
                    .into_iter()
                    .map(|(k, v)| (k, UserVar::from_json(path, v)))
                    .collect(),
            ),
        }
    }

    pub fn from_toml(mut value: toml_span::Value) -> Self {
        match value.take() {
            ValueInner::String(s) => Self::String(UserVarString::toml(s.into_owned(), value.span)),
            ValueInner::Integer(x) => Self::Scalar(x.into()),
            ValueInner::Float(x) => Self::Scalar(x.into()),
            ValueInner::Boolean(x) => Self::Scalar(x.into()),
            ValueInner::Array(array) => {
                Self::List(array.into_iter().map(UserVar::from_toml).collect())
            }
            ValueInner::Table(table) => Self::Object(
                table
                    .into_iter()
                    .map(|(k, v)| (k.name.to_string(), UserVar::from_toml(v)))
                    .collect(),
            ),
        }
    }

    pub fn into_minijinja(self) -> minijinja::Value {
        match self {
            UserVar::String(s) => s.value.into(),
            UserVar::Scalar(s) => s,
            UserVar::Object(o) => {
                let mut obj = HashMap::new();
                for (k, v) in o {
                    obj.insert(k, v.into_minijinja());
                }

                minijinja::Value::from_object(obj)
            }
            UserVar::List(l) => {
                minijinja::Value::from_iter(l.into_iter().map(|v| v.into_minijinja()))
            }
        }
    }

    fn strings_mut(&mut self) -> Vec<&mut UserVarString> {
        let mut strings = Vec::new();
        match self {
            UserVar::Scalar(_) => {}
            UserVar::String(s) => strings.push(s),
            UserVar::Object(o) => {
                for v in o.values_mut() {
                    strings.extend(v.strings_mut());
                }
            }
            UserVar::List(l) => {
                for v in l.iter_mut() {
                    strings.extend(v.strings_mut());
                }
            }
        }

        strings
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UserVarString {
    pub source: UserVarStringSource,
    pub value: String,
}

impl UserVarString {
    pub fn json(value: String, path: String) -> Self {
        Self {
            source: UserVarStringSource::Cli { path },
            value,
        }
    }

    pub fn toml(value: String, span: Span) -> Self {
        Self {
            source: UserVarStringSource::Toml(span),
            value,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UserVarStringSource {
    Cli { path: String },
    Toml(Span),
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
            config.artifacts().unwrap().path.value.to_string_lossy(),
            "artifacts"
        );
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
            UserVar::String(
                UserVarString::toml("{{ vade.app_paths.active_artifacts_dir }}/goatcounter serve -listen :{{ vade.app_port }}".to_string(), Span::new(126, 214))
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
            UserVar::List(vec![UserVar::String(UserVarString::toml(
                "goats.example.com".to_string(),
                Span::new(289, 306)
            ))])
        );
    }

    #[test]
    fn test_load_two_units() {
        let src = r#"
[[systemd-unit]]
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
        assert_eq!(config.systemd_units().len(), 2);
        assert!(config.caddyfile().is_none());

        let systemd_config = config.systemd_units().next().unwrap();
        assert_eq!(systemd_config.filename("my-app"), "my-app.service");
        assert_matches!(
            systemd_config.template.value.source.value,
            TemplateSource::Inline(_)
        );
        assert_eq!(systemd_config.template.value.vars.value.len(), 0);

        let systemd_config = config.systemd_units().nth(1).unwrap();
        assert_eq!(systemd_config.filename("my-app"), "my-app.timer");
        assert_matches!(
            systemd_config.template.value.source.value,
            TemplateSource::Inline(_)
        );
        assert_eq!(systemd_config.template.value.vars.value.len(), 0);
    }
}
