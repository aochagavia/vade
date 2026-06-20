use crate::config;
use crate::config::{
    AppConfig, ArtifactsConfig, CaddyfileConfig, NetworkConfig, SystemdUnitConfig, TemplateConfig,
    TemplateSource,
};
use miette::{LabeledSpan, NamedSource, Report, miette};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use toml_span::de_helpers::{TableHelper, expected};
use toml_span::value::ValueInner;
use toml_span::{DeserError, Deserialize, Error, ErrorKind, Value};

impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let artifacts = th.optional("artifacts");
        let network = th.optional("network").unwrap_or_default();
        let caddyfile = th.optional("caddyfile");
        let systemd_units = th.optional("systemd-unit").unwrap_or_default();
        th.finalize(None)?;

        Ok(AppConfig {
            artifacts,
            network,
            caddyfile,
            systemd_units,
        })
    }
}

impl<'de> Deserialize<'de> for ArtifactsConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        // Note: we defer using `?` to ensure we catch all possible errors
        let path = th.required::<String>("path");
        th.finalize(None)?;

        Ok(ArtifactsConfig {
            path: PathBuf::from(path?),
        })
    }
}

impl<'de> Deserialize<'de> for NetworkConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let reserve_ports = th.optional("reserve-ports").unwrap_or(0);
        th.finalize(None)?;

        Ok(NetworkConfig { reserve_ports })
    }
}

impl<'de> Deserialize<'de> for TemplateConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let span = value.span;
        let mut th = TableHelper::new(value)?;

        // We expect one of three possibilities
        let builtin = th.optional_s::<String>("builtin");
        let file = th.optional_s::<String>("file");
        let inline = th.optional_s::<String>("inline");
        let source = match (builtin, file, inline) {
            (Some(b), None, None) => Some(TemplateSource::Builtin(b.value)),
            (None, Some(f), None) => Some(TemplateSource::File(PathBuf::from(f.value))),
            (None, None, Some(i)) => Some(TemplateSource::Inline(i.value)),
            (None, None, None) => {
                th.errors.push(Error {
                    kind: ErrorKind::Custom(
                        "missing template source: set exactly one of `builtin`, `file`, or `inline`"
                            .into(),
                    ),
                    span,
                    line_info: None,
                });
                None
            }
            (b, f, i) => {
                for present in [b.map(|x| x.span), f.map(|x| x.span), i.map(|x| x.span)]
                    .into_iter()
                    .flatten()
                {
                    th.errors.push(Error {
                        kind: ErrorKind::Custom(
                            "conflicting template source: set only one of `builtin`, `file`, or `inline`"
                                .into(),
                        ),
                        span: present,
                        line_info: None,
                    });
                }
                None
            }
        };

        let vars = deserialize_vars_table(&mut th);

        th.finalize(None)?;

        Ok(TemplateConfig {
            source: source.expect("source is set when there are no errors"),
            vars,
        })
    }
}

impl<'de> Deserialize<'de> for SystemdUnitConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let enable = th.optional("enable").unwrap_or(true);
        let filename_suffix = th.optional("filename-suffix");
        let file_extension = th
            .optional("file-extension")
            .unwrap_or_else(|| "service".to_string());
        let template = th.required("template");
        th.finalize(None)?;

        Ok(SystemdUnitConfig {
            enable,
            filename_suffix,
            file_extension,
            template: template?,
        })
    }
}

impl<'de> Deserialize<'de> for CaddyfileConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let template = th.required("template");
        th.finalize(None)?;

        // safety: when `template` is an error, `finalize` would have returned early
        Ok(CaddyfileConfig {
            template: template.expect("template is set when there are no errors"),
        })
    }
}

pub fn to_report(err: DeserError, config_toml: &str, config_path: Option<&Path>) -> Report {
    let mut labels = Vec::new();
    for e in &err.errors {
        labels.extend(error_labels(e));
    }

    let report = miette!(labels = labels, "Failed to parse vade config file");
    let config_toml = config_toml.to_string();
    if let Some(path) = config_path
        && let Ok(path) = path.canonicalize()
    {
        let path = path.display().to_string();
        report.with_source_code(NamedSource::new(path, config_toml))
    } else {
        report.with_source_code(config_toml)
    }
}

fn error_labels(e: &Error) -> Vec<LabeledSpan> {
    match &e.kind {
        // For unknown keys, point at each offending key rather than at the enclosing table
        ErrorKind::UnexpectedKeys { keys, .. } => keys
            .iter()
            .map(|(name, span)| {
                LabeledSpan::new_primary_with_span(
                    Some(format!("unexpected key `{name}`")),
                    span.start..span.end,
                )
            })
            .collect(),
        // For duplicate keys, point also at the conflicting definition
        ErrorKind::DuplicateKey { first, .. } | ErrorKind::DuplicateTable { first, .. } => vec![
            LabeledSpan::new_primary_with_span(Some(e.to_string()), e.span.start..e.span.end),
            LabeledSpan::new_with_span(
                Some("first defined here".to_string()),
                first.start..first.end,
            ),
        ],
        _ => vec![LabeledSpan::new_primary_with_span(
            Some(e.to_string()),
            e.span.start..e.span.end,
        )],
    }
}

/// Reads the optional `vars` table, converting each value into a `minijinja::Value`
///
/// Errors are recorded to the helper, instead of explicitly returned
fn deserialize_vars_table(th: &mut TableHelper) -> HashMap<String, minijinja::Value> {
    let Some((_, mut value)) = th.take("vars") else {
        return HashMap::new();
    };

    match value.take() {
        ValueInner::Table(table) => table
            .into_iter()
            .map(|(k, mut v)| (k.name.into_owned(), config::value_to_minijinja(v.take())))
            .collect(),
        other => {
            th.errors.push(expected("a table", other, value.span));
            HashMap::new()
        }
    }
}
