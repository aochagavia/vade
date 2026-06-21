use crate::config;
use crate::config::{
    AppConfig, ArtifactsConfig, CaddyfileConfig, NetworkConfig, SystemdUnitConfig, TemplateConfig,
    TemplateSource,
};
use miette::{LabeledSpan, Report, SourceCode, miette};
use std::collections::HashMap;
use std::path::PathBuf;
use toml_span::de_helpers::{TableHelper, expected};
use toml_span::value::ValueInner;
use toml_span::{DeserError, Deserialize, Error, ErrorKind, Spanned, Value};

impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let artifacts = th.optional_s("artifacts");
        let network = th.optional_s("network");
        let caddyfile = th.optional_s("caddyfile");
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
        let path = th.required_s::<String>("path");
        th.finalize(None)?;

        Ok(ArtifactsConfig { path: path?.map() })
    }
}

impl<'de> Deserialize<'de> for NetworkConfig {
    fn deserialize(value: &mut Value<'de>) -> Result<Self, DeserError> {
        let mut th = TableHelper::new(value)?;
        let reserve_ports = th.optional_s("reserve-ports");
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
            (Some(b), None, None) => {
                Some(Spanned::with_span(TemplateSource::Builtin(b.value), b.span))
            }
            (None, Some(f), None) => Some(Spanned::with_span(
                TemplateSource::File(PathBuf::from(f.value)),
                f.span,
            )),
            (None, None, Some(i)) => {
                Some(Spanned::with_span(TemplateSource::Inline(i.value), i.span))
            }
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
        let enable = th.optional_s("enable");
        let filename_suffix = th.optional_s("filename-suffix");
        let file_extension = th.optional_s("file-extension");
        let template = th.required_s("template");
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
        let template = th.required_s("template");
        th.finalize(None)?;

        // safety: when `template` is an error, `finalize` would have returned early
        Ok(CaddyfileConfig {
            template: template.expect("template is set when there are no errors"),
        })
    }
}

pub fn toml_error_to_report<S: SourceCode + 'static>(err: DeserError, source: S) -> Report {
    let mut labels = Vec::new();
    for e in &err.errors {
        labels.extend(error_labels(e));
    }

    miette!(labels = labels, "Failed to parse vade config file").with_source_code(source)
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
/// Errors are recorded to the helper, instead of explicitly returned.
fn deserialize_vars_table(th: &mut TableHelper) -> Spanned<HashMap<String, minijinja::Value>> {
    let Some((_, mut value)) = th.take("vars") else {
        return Spanned::new(HashMap::new());
    };

    let span = value.span;
    match value.take() {
        ValueInner::Table(table) => Spanned::with_span(
            table
                .into_iter()
                .map(|(k, mut v)| (k.name.into_owned(), config::value_to_minijinja(v.take())))
                .collect(),
            span,
        ),
        other => {
            th.errors.push(expected("a table", other, span));
            Spanned::with_span(HashMap::new(), span)
        }
    }
}
