use crate::app_deployment::AppDeployment;
use crate::app_name::AppName;
use miette::{LabeledSpan, NamedSource, Report, bail, miette};
use minijinja::{Environment, UndefinedBehavior, context};
use serde::de::Error;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

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
    env.set_undefined_behavior(UndefinedBehavior::Strict);

    // Debug mode is necessary to get the necessary error information from minijinja into miette
    env.set_debug(true);
    env.add_template_owned("deploy-promote.sh.j2", PROMOTE_SCRIPT_TEMPLATE)
        .map_err(|e| minijinja_error_to_report(&e))?;
    env.add_template_owned("shared/header.py.j2", HEADER_TEMPLATE)
        .map_err(|e| minijinja_error_to_report(&e))?;
    env.add_template_owned("shared/create-tasks.py.j2", CREATE_TASKS_TEMPLATE)
        .map_err(|e| minijinja_error_to_report(&e))?;

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

pub fn render(
    env: &mut Environment,
    context: &minijinja::Value,
    template_name: &'static str,
    template: Cow<'static, str>,
) -> Result<String, Report> {
    const MAX_ITERATIONS: usize = 10;

    // We re-render the results of the previous render until we reach a fixpoint. This is to allow
    // for using variables inside other jinja variables.
    let mut template_string = template.to_string();
    for _ in 0..MAX_ITERATIONS {
        env.add_template_owned(template_name, template_string.clone())
            .map_err(|e| minijinja_error_to_report(&e))?;

        // safety: we just added the template to the environment
        let template = env.get_template(template_name).unwrap();

        let rendered = match template.render(context) {
            Ok(s) => s,
            Err(e) => return Err(minijinja_error_to_report(&e)),
        };

        if template_string == rendered {
            return Ok(rendered);
        } else {
            template_string = rendered;
        }
    }

    bail!(
        "failed to reach a stable rendering of the template after {MAX_ITERATIONS} iterations, did you accidentally introduce infinite recursion in your template variables?"
    );
}

fn minijinja_error_to_report(error: &minijinja::Error) -> Report {
    let message = match error.detail() {
        Some(detail) => format!("{}: {detail}", error.kind()),
        None => error.kind().to_string(),
    };

    if let (Some(source), Some(range)) = (error.template_source(), error.range()) {
        let name = error.name().unwrap_or("template");
        let hint = error_hint(error.kind(), source, range.clone());
        let label = LabeledSpan::new_primary_with_span(Some(message), range);
        let report = match hint {
            Some(hint) => miette!(
                labels = vec![label],
                help = hint,
                "failed to render template"
            ),
            None => miette!(labels = vec![label], "failed to render template"),
        };
        report.with_source_code(NamedSource::new(name, source.to_string()))
    } else {
        let location = match (error.name(), error.line()) {
            (Some(name), Some(line)) => format!(" (in {name}:{line})"),
            (Some(name), None) => format!(" (in {name})"),
            _ => String::new(),
        };
        miette!("failed to render template: {message}{location}")
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
        "`{key}` is a user-defined variable. Declare it in your `vade.toml` under the relevant \
         template's `vars`, e.g. `vars = {{ {key} = ... }}`."
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_deployment::SystemdUnit;
    use crate::config::{TemplateAndUserVars, UserVars};
    use crate::util::ResolvedPath;
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

    fn test_render_deploy(app_deployment: &AppDeployment) {
        let context = base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            Some(app_deployment),
        );
        let mut env = base_minijinja_env().unwrap();
        render(&mut env, &context, "", DEPLOY_TEMPLATE.into()).unwrap();
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
        let deployment = AppDeployment {
            artifacts: Some(ResolvedPath::from_str("/my/local/artifacts")),
            caddyfile: Some(TemplateAndUserVars {
                template: "Believe me, I'm a Caddyfile -.-".to_string(),
                user_vars: Default::default(),
            }),
            systemd_units: vec![SystemdUnit {
                name: "foo.service".to_string(),
                template: TemplateAndUserVars {
                    template: "Believe me, I'm a systemd unit :)".to_string(),
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
        let context = base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            None,
        );
        let env = base_minijinja_env().unwrap();
        let template = env.get_template("shared/create-tasks.py.j2").unwrap();
        template.render(context).unwrap();
    }

    #[test]
    fn test_render_error() {
        let context = base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            None,
        );
        let mut env = base_minijinja_env().unwrap();

        let template = "hello {{ does_not_exist }}";
        let Err(err) = render(&mut env, &context, "unit_file", template.into()) else {
            panic!("expected rendering to fail");
        };

        insta::assert_snapshot!(render_report(&err), @"
         × failed to render template
          ╭─[unit_file:1:10]
        1 │ hello {{ does_not_exist }}
          ·          ───────┬──────
          ·                 ╰── undefined value
          ╰────
        ");
    }

    #[test]
    fn test_render_error_hints_at_missing_user_var() {
        let context = base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            None,
        );
        let mut env = base_minijinja_env().unwrap();

        // `vars.exec_start` is referenced but never declared in the (empty) `vars` namespace
        let context = context! { vars => UserVars::default().to_minijinja(), ..context };
        let template = "ExecStart={{ vars.exec_start }}";
        let Err(err) = render(&mut env, &context, "unit_file", template.into()) else {
            panic!("expected rendering to fail");
        };

        // The rendered diagnostic points at the offending expression and hints at declaring the
        // user variable in `vade.toml`.
        insta::assert_snapshot!(render_report(&err), @"
         × failed to render template
          ╭─[unit_file:1:14]
        1 │ ExecStart={{ vars.exec_start }}
          ·              ───────┬───────
          ·                     ╰── undefined value
          ╰────
         help: `exec_start` is a user-defined variable. Declare it in your
               `vade.toml` under the relevant template's `vars`, e.g. `vars =
               { exec_start = ... }`.
        ");
    }
}
