use crate::app_deployment::AppDeployment;
use crate::app_name::AppName;
use crate::config::{UserVarString, UserVarStringSource};
use crate::util::{diagnostic, diagnostic_with_help};
use miette::{NamedSource, Report};
use minijinja::{Environment, UndefinedBehavior, context};
use serde::de::Error;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::path::Path;
use std::string::ToString;

pub fn base_minijinja_context(
    out_dir_abs: &Path,
    app_name: Option<&AppName>,
    deployment: Option<&AppDeployment>,
) -> minijinja::Value {
    let out_dir_abs_str = out_dir_abs.to_string_lossy();

    let mut variables: Vec<(_, minijinja::Value)> = vec![
        (
            "vade.internal.assign_ports_script.local",
            out_dir_abs.join("assign-ports.py").to_string_lossy().into(),
        ),
        (
            "vade.internal.assign_ports_script.remote",
            "/opt/vade/scripts/assign-ports.py".into(),
        ),
    ];

    let Some(app_name) = app_name else {
        return build_minijinja_value(variables);
    };

    let username = app_name.as_str();
    let home_dir = &format!("/opt/vade/apps/{}", app_name.as_str());
    let active_deployment = &format!("{home_dir}/active-deployment");
    let candidate_deployment = &format!("{home_dir}/candidate-deployment");
    variables.extend([
        ("vade.app.name", app_name.as_str().into()),
        ("vade.app.username", username.into()),
        ("vade.app.paths.home", home_dir.into()),
        (
            "vade.app.paths.secrets",
            format!("{home_dir}/secrets").into(),
        ),
        (
            "vade.app.paths.storage",
            format!("{home_dir}/storage").into(),
        ),
        ("vade.app.paths.active_deployment", active_deployment.into()),
        (
            "vade.app.paths.previous_deployment",
            format!("{home_dir}/previous-deployment").into(),
        ),
        (
            "vade.app.paths.candidate_deployment",
            candidate_deployment.into(),
        ),
        (
            "vade.app.paths.active_systemd_unit_copies",
            format!("{active_deployment}/systemd-unit-copies").into(),
        ),
        (
            "vade.app.artifacts.active",
            format!("{active_deployment}/artifacts").into(),
        ),
        (
            "vade.app.artifacts.candidate",
            format!("{candidate_deployment}/artifacts").into(),
        ),
        (
            "vade.app.caddyfile.active",
            format!("{active_deployment}/Caddyfile").into(),
        ),
    ]);

    let Some(deployment) = deployment else {
        return build_minijinja_value(variables);
    };

    let mut units = Vec::new();
    for unit in &deployment.systemd_units {
        units.push(context! {
            name => unit.name,
            local_path => format!("{out_dir_abs_str}/{}", unit.name),
            candidate_path => format!("{candidate_deployment}/systemd-unit-copies/{}", unit.name),
            active_path => format!("{active_deployment}/systemd-unit-copies/{}", unit.name),
            installed_path => format!("/etc/systemd/system/{}", unit.name)
        })
    }

    variables.extend([("vade.app.systemd_units", units.into())]);

    if let Some(artifacts_dir) = &deployment.artifacts {
        variables.extend([(
            "vade.app.artifacts.local",
            artifacts_dir.to_string_lossy().into(),
        )]);
    }

    if deployment.caddyfile.is_some() {
        variables.extend([
            (
                "vade.app.caddyfile.local",
                out_dir_abs.join("Caddyfile").to_string_lossy().into(),
            ),
            (
                "vade.app.caddyfile.candidate",
                format!("{candidate_deployment}/Caddyfile").into(),
            ),
        ])
    }

    build_minijinja_value(variables)
}

pub fn base_minijinja_env() -> Result<Environment<'static>, Report> {
    let mut env = Environment::new();

    // Undefined variables always result in an error
    env.set_undefined_behavior(UndefinedBehavior::Strict);

    // Debug mode is necessary to get diagnostics from minijinja into miette
    env.set_debug(true);

    if let Err(e) = env.add_template_owned("deploy-promote.sh.j2", PROMOTE_SCRIPT_TEMPLATE) {
        unreachable!("{e:?}")
    }
    if let Err(e) = env.add_template_owned("shared/header.py.j2", HEADER_TEMPLATE) {
        unreachable!("{e:?}")
    }
    if let Err(e) = env.add_template_owned("shared/create-tasks.py.j2", CREATE_TASKS_TEMPLATE) {
        unreachable!("{e:?}")
    }

    fn dirname(path: &str) -> Result<String, minijinja::Error> {
        let path = Path::new(path)
            .parent()
            .ok_or(minijinja::Error::custom("path did not have a parent"))?;
        Ok(path.display().to_string())
    }
    env.add_filter("dirname", dirname);

    fn port(name: &str) -> Result<String, minijinja::Error> {
        if name.chars().any(|c| c == '"') {
            return Err(minijinja::Error::custom(
                r#"port names may not contain the `"` character "#,
            ));
        }

        Ok(format!(r#"{{{{ port("{name}") }}}}"#))
    }
    env.add_function("port", port);

    Ok(env)
}

pub fn render_user_var_string(
    env: &mut Environment,
    context: &minijinja::Value,
    toml_config: &TomlSource,
    user_var_string: &UserVarString,
) -> Result<String, Report> {
    let error_to_report = |e: minijinja::Error| -> Report {
        minijinja_error_to_report(
            &e,
            "failed to render user-provided string",
            Some(toml_config),
            &TemplateSource::from_user_var(user_var_string),
        )
    };

    env.add_template_owned("tmp", user_var_string.value.to_string())
        .map_err(error_to_report)?;

    // safety: we just added the template to the environment
    let template = env.get_template("tmp").unwrap();
    template.render(context).map_err(error_to_report)
}

pub fn render_user_template(
    env: &mut Environment,
    context: &minijinja::Value,
    toml_config: &TomlSource,
    template: &TemplateSource,
    error_msg: &str,
) -> Result<String, Report> {
    let error_to_report = |e: minijinja::Error| -> Report {
        minijinja_error_to_report(&e, error_msg, Some(toml_config), template)
    };

    env.add_template_owned("tmp", template.value.clone())
        .map_err(error_to_report)?;

    // safety: we just added the template to the environment
    let template = env.get_template("tmp").unwrap();
    template.render(context).map_err(error_to_report)
}

pub fn render_internal(
    env: &mut Environment,
    context: &minijinja::Value,
    template_id: &str,
    template: &str,
) -> Result<String, Report> {
    let error_to_report = |e: minijinja::Error| -> Report {
        let template = TemplateSource {
            value: template.to_string(),
            meta: TemplateSourceMeta::Builtin {
                id: template_id.to_string(),
                kind: BuiltinTemplateKind::Internal,
            },
        };
        minijinja_error_to_report(
            &e,
            "failed to render internal template (this is a bug)",
            None,
            &template,
        )
    };

    env.add_template_owned("tmp", template.to_string())
        .map_err(error_to_report)?;

    // safety: we just added the template to the environment
    let template = env.get_template("tmp").unwrap();
    template.render(context).map_err(error_to_report)
}

pub struct TemplateSource {
    /// The template string
    value: String,
    /// Metadata about where the template came from (e.g., a file, inline configuration, etc.)
    meta: TemplateSourceMeta,
}

impl TemplateSource {
    pub fn builtin(id: String, kind: BuiltinTemplateKind, value: String) -> Self {
        Self {
            value,
            meta: TemplateSourceMeta::Builtin { id, kind },
        }
    }

    pub fn file(path: String, value: String) -> Self {
        Self {
            value,
            meta: TemplateSourceMeta::File { path },
        }
    }

    pub fn inline(span: toml_span::Span, value: String) -> Self {
        Self {
            value,
            meta: TemplateSourceMeta::Inline { span },
        }
    }

    fn from_user_var(user_var: &UserVarString) -> TemplateSource {
        let meta = match &user_var.source {
            UserVarStringSource::Cli { path } => TemplateSourceMeta::Cli {
                hint: format!(
                    "this template string was assigned to `{path}` through the `--var-json` flag"
                ),
            },
            UserVarStringSource::Toml(span) => TemplateSourceMeta::Inline { span: *span },
        };

        TemplateSource {
            value: user_var.value.clone(),
            meta,
        }
    }
}

pub enum TemplateSourceMeta {
    Builtin {
        id: String,
        kind: BuiltinTemplateKind,
    },
    File {
        path: String,
    },
    Inline {
        span: toml_span::Span,
    },
    Cli {
        hint: String,
    },
}

#[derive(Copy, Clone)]
pub enum BuiltinTemplateKind {
    Caddyfile,
    SystemdUnit,
    Internal,
}

impl BuiltinTemplateKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BuiltinTemplateKind::Caddyfile => "Caddyfile",
            BuiltinTemplateKind::SystemdUnit => "systemd unit",
            BuiltinTemplateKind::Internal => "internal",
        }
    }
}

impl Display for BuiltinTemplateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A TOML source file and the filesystem path it was read from
pub struct TomlSource {
    pub path: String,
    pub value: String,
}

impl TomlSource {
    pub fn to_named_source(&self) -> NamedSource<String> {
        NamedSource::new(self.path.clone(), self.value.clone())
    }
}

fn minijinja_error_to_report(
    error: &minijinja::Error,
    root_error: &str,
    config_toml: Option<&TomlSource>,
    source: &TemplateSource,
) -> Report {
    let message = match error.detail() {
        Some(detail) => format!("{}: {detail}", error.kind()),
        None => error.kind().to_string(),
    };

    // minijinja doesn't guarantee a range will be available for all errors
    let range = error.range().unwrap_or(0..source.value.len());

    match &source.meta {
        TemplateSourceMeta::Cli { hint } => {
            let hint =
                if let Some(extra_hint) = error_hint(error.kind(), &source.value, range.clone()) {
                    format!("1. {hint}\n2. {extra_hint}")
                } else {
                    hint.clone()
                };
            diagnostic_with_help(
                root_error,
                message,
                hint,
                range.into(),
                source.value.clone(),
            )
        }
        TemplateSourceMeta::Inline { span } => {
            // inline templates come from config toml, so it will always be provided in this match arm
            let config_toml = config_toml.unwrap();
            let miette_source = config_toml.to_named_source();

            // The `range` tells us where in the string the error is
            // The `span` tells us where in the config toml the string is
            let hint = error_hint(error.kind(), &source.value, range.clone());
            let range = span.start + range.start..span.start + range.end;
            match hint {
                Some(hint) => {
                    diagnostic_with_help(root_error, message, hint, range.into(), miette_source)
                }
                None => diagnostic(root_error, message, range.into(), miette_source),
            }
        }
        TemplateSourceMeta::Builtin { .. } | TemplateSourceMeta::File { .. } => {
            let (named_source, internal) = match &source.meta {
                TemplateSourceMeta::Builtin { id, kind } => (
                    NamedSource::new(
                        format!("{id} (builtin {} template)", kind.as_str()),
                        source.value.clone(),
                    ),
                    matches!(kind, BuiltinTemplateKind::Internal),
                ),
                TemplateSourceMeta::File { path } => {
                    (NamedSource::new(path.clone(), source.value.clone()), false)
                }
                _ => unreachable!(),
            };

            let hint = if internal {
                Some(
                    "this is an internal error in vade, please report it so we can fix it"
                        .to_string(),
                )
            } else {
                error_hint(error.kind(), &source.value, range.clone())
            };

            match hint {
                Some(hint) => {
                    diagnostic_with_help(root_error, message, hint, range.into(), named_source)
                }
                None => diagnostic(root_error, message, range.into(), named_source),
            }
        }
    }
}

fn error_hint(
    kind: minijinja::ErrorKind,
    source: &str,
    range: std::ops::Range<usize>,
) -> Option<String> {
    let snippet = source.get(range.clone())?;
    missing_user_var_hint(kind, snippet)
}

// When an undefined-variable error refers to a `vars.*` expression, returns a hint telling the user
// to declare that variable in their `vade.toml`.
fn missing_user_var_hint(kind: minijinja::ErrorKind, expr: &str) -> Option<String> {
    if kind != minijinja::ErrorKind::UndefinedError {
        return None;
    }

    // Extract the top-level key out of an expression like `vars.foo`, `vars.foo.bar` or `vars.foo[0]`
    let key: String = expr
        .trim()
        .strip_prefix("vars.")?
        .chars()
        .take_while(|c| *c != '.' && *c != '[')
        .collect();
    if key.is_empty() {
        return None;
    }

    Some(format!(
        "`{key}` is a user-defined variable. Declare it in your `vade.toml` file under the relevant \
         template's `vars`, e.g. `vars = {{ {key} = ... }}`, or inject it through the CLI using \
         the `--var-json` option."
    ))
}

// Variable names use dotted paths (e.g. `vade.app.name`) to express nesting. Here we transform
// them into an actual tree.
fn build_minijinja_value(vars: Vec<(&str, minijinja::Value)>) -> minijinja::Value {
    enum Node {
        Leaf(minijinja::Value),
        Branch(BTreeMap<String, Node>),
    }

    fn insert(map: &mut BTreeMap<String, Node>, path: &str, value: minijinja::Value) {
        match path.split_once('.') {
            Some((head, rest)) => match map
                .entry(head.to_string())
                .or_insert_with(|| Node::Branch(BTreeMap::new()))
            {
                Node::Branch(children) => insert(children, rest, value),
                Node::Leaf(_) => unreachable!("conflicting variable path `{path}`"),
            },
            None => {
                map.insert(path.to_string(), Node::Leaf(value));
            }
        }
    }

    fn into_value(map: BTreeMap<String, Node>) -> minijinja::Value {
        minijinja::Value::from(
            map.into_iter()
                .map(|(key, node)| match node {
                    Node::Leaf(value) => (key, value),
                    Node::Branch(children) => (key, into_value(children)),
                })
                .collect::<BTreeMap<_, _>>(),
        )
    }

    let mut root = BTreeMap::new();
    for (path, value) in vars {
        insert(&mut root, path, value);
    }

    into_value(root)
}

// Caddyfile templates
pub static CADDYFILE_STATIC_FILES: &str =
    include_str!("resources/caddyfile-templates/static-files.j2");
pub static CADDYFILE_REVERSE_PROXY: &str =
    include_str!("resources/caddyfile-templates/reverse-proxy.j2");

// Systemd unit templates
pub static SYSTEMD_WEBAPP_SERVICE: &str =
    include_str!("resources/systemd-unit-templates/webapp.service.j2");

// Building blocks
static HEADER_TEMPLATE: &str = include_str!("resources/pyinfra-templates/shared/header.py.j2");
static CREATE_TASKS_TEMPLATE: &str =
    include_str!("resources/pyinfra-templates/shared/create-tasks.py.j2");
static PROMOTE_SCRIPT_TEMPLATE: &str =
    include_str!("resources/pyinfra-templates/deploy-promote.sh.j2");

// Full deploys
pub static DEPLOY_TEMPLATE: &str = include_str!("resources/pyinfra-templates/deploy.py.j2");
pub static CREATE_TEMPLATE: &str = include_str!("resources/pyinfra-templates/create.py.j2");
pub static SERVER_SETUP_TEMPLATE: &str =
    include_str!("resources/pyinfra-templates/server-setup.py.j2");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_deployment::SystemdUnit;
    use crate::config::{TemplateAndUserVars, UserVars};
    use crate::util::ResolvedPath;
    use minijinja::Value;
    use std::str::FromStr;

    // Renders a miette report (for use in insta snapshots)
    fn render_report(err: &Report) -> String {
        let mut out = String::new();
        miette::GraphicalReportHandler::new()
            .with_theme(miette::GraphicalTheme::unicode_nocolor())
            .with_width(80)
            .render_report(&mut out, &**err)
            .unwrap();
        out
    }

    fn get_test_minijinja_context(deployment: Option<&AppDeployment>) -> Value {
        base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            deployment,
        )
    }

    fn test_render_deploy(app_deployment: &AppDeployment) {
        let context = get_test_minijinja_context(Some(app_deployment));
        let mut env = base_minijinja_env().unwrap();
        render_internal(&mut env, &context, "deploy", DEPLOY_TEMPLATE).unwrap();
    }

    #[test]
    fn test_render_deploy_minimal() {
        let deployment = AppDeployment {
            artifacts: None,
            caddyfile: None,
            systemd_units: vec![],
        };
        test_render_deploy(&deployment);
    }

    #[test]
    fn test_render_deploy_full() {
        fn template_src(value: &str) -> TemplateSource {
            TemplateSource {
                value: value.to_string(),
                meta: TemplateSourceMeta::File {
                    path: "".to_string(),
                },
            }
        }

        let deployment = AppDeployment {
            artifacts: Some(ResolvedPath::from_str("/my/local/artifacts")),
            caddyfile: Some(TemplateAndUserVars {
                source: template_src("Believe me, I'm a Caddyfile -.-"),
                user_vars: Default::default(),
            }),
            systemd_units: vec![SystemdUnit {
                name: "foo.service".to_string(),
                template: TemplateAndUserVars {
                    source: template_src("Believe me, I'm a systemd unit :)"),
                    user_vars: Default::default(),
                },
            }],
        };
        test_render_deploy(&deployment);
    }

    #[test]
    fn test_missing_user_var_hint() {
        use minijinja::ErrorKind;

        // Only undefined errors in the `vars` namespace produce a hint
        assert!(missing_user_var_hint(ErrorKind::UndefinedError, "vars.domains").is_some());
        assert!(missing_user_var_hint(ErrorKind::UndefinedError, "vars.foo.bar").is_some());
        assert!(missing_user_var_hint(ErrorKind::UndefinedError, "vars.list[0]").is_some());

        // Not the `vars` namespace
        assert!(missing_user_var_hint(ErrorKind::UndefinedError, "vade.app.unknown").is_none());
        assert!(missing_user_var_hint(ErrorKind::UndefinedError, "vars").is_none());
        // A different error kind (e.g. a syntax error) shouldn't trigger the hint
        assert!(missing_user_var_hint(ErrorKind::SyntaxError, "vars.domains").is_none());
    }

    #[test]
    fn test_render_create() {
        let context = get_test_minijinja_context(None);
        let env = base_minijinja_env().unwrap();
        let template = env.get_template("shared/create-tasks.py.j2").unwrap();
        template.render(context).unwrap();
    }

    #[test]
    fn test_render_error() {
        let context = get_test_minijinja_context(None);
        let mut env = base_minijinja_env().unwrap();

        let template = "hello {{ does_not_exist }}";
        let Err(err) = render_user_template(
            &mut env,
            &context,
            &TomlSource {
                path: "/path/to/vade.toml".to_string(),
                value: "# dummy toml file".to_string(),
            },
            &TemplateSource::file("tmp".to_string(), template.into()),
            "failed to render template",
        ) else {
            panic!("expected rendering to fail");
        };

        insta::assert_snapshot!(render_report(&err), @"
         × failed to render template
          ╭─[tmp:1:10]
        1 │ hello {{ does_not_exist }}
          ·          ───────┬──────
          ·                 ╰── undefined value
          ╰────
        ");
    }

    #[test]
    fn test_render_error_hints_at_missing_user_var() {
        let context = get_test_minijinja_context(None);
        let mut env = base_minijinja_env().unwrap();

        // `vars.exec_start` is referenced but never declared in the (empty) `vars` namespace
        let context = context! { vars => UserVars::default().into_minijinja(), ..context };
        let template = "ExecStart={{ vars.exec_start }}";
        let Err(err) = render_user_template(
            &mut env,
            &context,
            &TomlSource {
                path: "/path/to/vade.toml".to_string(),
                value: "# dummy toml file".to_string(),
            },
            &TemplateSource::file("main.service".to_string(), template.into()),
            "failed to render template",
        ) else {
            panic!("expected rendering to fail");
        };

        // The rendered diagnostic points at the offending expression and hints at declaring the
        // user variable in `vade.toml`.
        insta::assert_snapshot!(render_report(&err), @"
         × failed to render template
          ╭─[main.service:1:14]
        1 │ ExecStart={{ vars.exec_start }}
          ·              ───────┬───────
          ·                     ╰── undefined value
          ╰────
         help: `exec_start` is a user-defined variable. Declare it in your
               `vade.toml` file under the relevant template's `vars`, e.g. `vars =
               { exec_start = ... }`, or inject it through the CLI using the `--var-
               json` option.
        ");
    }
}
