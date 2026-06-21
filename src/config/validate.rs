use crate::config::{AppConfig, SystemdUnitConfig};
use miette::{LabeledSpan, Report, SourceCode, miette};
use std::collections::HashMap;
use toml_span::{Span, Spanned};

const ALL_SYSTEMD_UNIT_EXTENSIONS: [&str; 11] = [
    "service",
    "socket",
    "device",
    "mount",
    "automount",
    "swap",
    "target",
    "path",
    "timer",
    "slice",
    "scope",
];

pub struct ValidationError {
    span: Span,
    message: String,
}

pub struct ValidationErrors {
    errors: Vec<ValidationError>,
}

impl ValidationErrors {
    pub fn into_report<S: SourceCode + 'static>(self, source: S) -> Report {
        let labels = self
            .errors
            .into_iter()
            .map(|e| LabeledSpan::new_primary_with_span(Some(e.message), e.span.start..e.span.end))
            .collect::<Vec<_>>();
        miette!(labels = labels, "Invalid vade config file").with_source_code(source)
    }
}

/// Runs all semantic checks on a structurally-valid [`AppConfig`]
pub fn validate(app_name: &str, config: &AppConfig) -> Result<(), ValidationErrors> {
    let mut errors = Vec::new();

    check_unit_filenames(app_name, &config.systemd_units, &mut errors);
    check_duplicate_unit_filenames(app_name, &config.systemd_units, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        // Report in source order
        errors.sort_by_key(|e| e.span.start);
        Err(ValidationErrors { errors })
    }
}

fn check_unit_filenames(
    app_name: &str,
    systemd_units: &[Spanned<SystemdUnitConfig>],
    errors: &mut Vec<ValidationError>,
) {
    for unit in systemd_units {
        if let Some(filename_suffix) = &unit.value.filename_suffix {
            let invalid_chars = filename_suffix
                .value
                .chars()
                .filter(|c| !c.is_ascii_alphanumeric() && ![':', '-', '_', '.', '\\'].contains(c))
                .map(|c| c.to_string())
                .collect::<Vec<_>>();

            if !invalid_chars.is_empty() {
                errors.push(ValidationError {
                    span: filename_suffix.span,
                    message: format!(
                        "the following characters are not allowed in a systemd unit filename: {}",
                        invalid_chars.join(", ")
                    ),
                })
            }
        }

        if let Some(file_extension) = &unit.value.file_extension
            && !ALL_SYSTEMD_UNIT_EXTENSIONS.contains(&file_extension.value.as_str())
        {
            let valid_extensions = ALL_SYSTEMD_UNIT_EXTENSIONS.join(", ");
            errors.push(ValidationError {
                    span: file_extension.span,
                    message: format!("`{}` is not a valid systemd unit extension (valid extensions are: {valid_extensions})", file_extension.value),
                })
        }

        let name = unit.value.filename(app_name);
        if name.len() > 255 {
            errors.push(ValidationError {
                span: unit.span,
                message: format!("The total length of the unit name must not exceed 255 characters, but it is currently {} characters", name.len())
            })
        }
    }
}

fn check_duplicate_unit_filenames(
    app_name: &str,
    systemd_units: &[Spanned<SystemdUnitConfig>],
    errors: &mut Vec<ValidationError>,
) {
    let mut spans_by_filename: HashMap<_, Vec<_>> = HashMap::new();
    for unit in systemd_units {
        let key = unit.value.filename(app_name);
        spans_by_filename.entry(key).or_default().push(unit.span);
    }

    for (filename, spans) in spans_by_filename {
        if spans.len() < 2 {
            continue;
        }

        for span in spans {
            errors.push(ValidationError {
                    message: format!("systemd unit filename `{filename}` is declared multiple times, you can use the `filename-suffix` and `file-extension` properties to differentiate between them"),
                span,
            });
        }
    }
}
