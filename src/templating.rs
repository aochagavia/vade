use crate::app_deployment::AppDeployment;
use crate::app_name::AppName;
use minijinja::{Environment, Template, UndefinedBehavior, context};
use rootcause::{Report, bail};
use serde::de::Error;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

pub struct TemplateAndUserVars {
    pub template: String,
    pub user_vars: HashMap<String, minijinja::Value>,
}

pub fn base_minijinja_context(
    out_dir_abs: &Path,
    app_name: Option<&AppName>,
    deployment: Option<&AppDeployment>,
) -> minijinja::Value {
    let out_dir_abs_str = out_dir_abs.to_string_lossy();

    let mut variables: Vec<(_, minijinja::Value)> = vec![
        (
            "vade.internal.reserve_ports_script.local",
            out_dir_abs
                .join("reserve-ports.py")
                .to_string_lossy()
                .into(),
        ),
        (
            "vade.internal.reserve_ports_script.remote",
            "/opt/vade/scripts/reserve-ports.py".into(),
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
        (
            "vade.app.paths.system_users_entry",
            format!("/opt/vade/system_users/{username}").into(),
        ),
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
            format!("{active_deployment}/systemd_unit_copies").into(),
        ),
        (
            "vade.app.artifacts.active",
            format!("{active_deployment}/artifacts").into(),
        ),
        (
            "vade.app.caddyfile.active",
            format!("{active_deployment}/Caddyfile").into(),
        ),
    ]);

    let Some(deployment) = deployment else {
        return build_minijinja_value(variables);
    };

    // The actual port number for each reserved port is only known at deploy time, on the
    // server. For that reason, port variables resolve to themselves: they will stay in the
    // file even after rendering, so the variable can be replaced on the server.
    let app_ports_values: Vec<_> = (0..deployment.reserved_ports)
        .map(|i| format!("{{{{ {APP_PORTS_VAR}[{i}] }}}}"))
        .collect();

    variables.extend([(APP_PORTS_VAR, app_ports_values.into())]);

    if deployment.reserved_ports > 0 {
        variables.extend([(APP_PORT_VAR, format!("{{{{ {APP_PORT_VAR} }}}}").into())]);
    }

    let mut units = Vec::new();
    for unit in &deployment.systemd_units {
        units.push(context! {
            name => unit.name,
            local_path => format!("{out_dir_abs_str}/{}", unit.name),
            candidate_path => format!("{candidate_deployment}/systemd_unit_copies/{}", unit.name),
            active_path => format!("{active_deployment}/systemd_unit_copies/{}", unit.name),
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
    env.add_template_owned("deploy-promote.sh.j2", PROMOTE_SCRIPT_TEMPLATE)?;
    env.add_template_owned("header.py.j2", HEADER_TEMPLATE)?;
    env.add_template_owned("create-tasks.py.j2", SETUP_TASKS_TEMPLATE)?;
    env.add_template_owned("deploy-tasks.py.j2", DEPLOY_TASKS_TEMPLATE)?;

    fn dirname(path: &str) -> Result<String, minijinja::Error> {
        let path = Path::new(path)
            .parent()
            .ok_or(minijinja::Error::custom("path did not have a parent"))?;
        Ok(path.display().to_string())
    }
    env.add_filter("dirname", dirname);

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
    for i in 0..MAX_ITERATIONS {
        let template_name = format!("{template_name}{i}");
        env.add_template_owned(template_name.clone(), template_string.clone())?;

        // safety: we just added the template to the environment
        let template = env.get_template(&template_name).unwrap();

        // raise a user-friendly error if there are undefined variables as a consequence of a bad
        // configuration
        if let Some((variable, message)) = explain_known_missing_vars(&template, context) {
            bail!("The template uses variable `{variable}`, but {message}");
        }

        let rendered = match template.render(context) {
            Ok(s) => s,
            Err(e) => {
                bail!("{}\n{}", e, e.display_debug_info());
            }
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

fn explain_known_missing_vars(
    template: &Template,
    context: &minijinja::Value,
) -> Option<(String, String)> {
    let mut context_keys = HashSet::new();
    if let Some(context) = context.as_object()
        && let Some(iterator) = context.try_iter_pairs()
    {
        for (key, _) in iterator {
            if let Some(key) = key.as_str() {
                context_keys.insert(key.to_string());
            }
        }
    }

    let expected_names = template.undeclared_variables(false);
    for used_name in expected_names {
        if context_keys.contains(&used_name) {
            // Variable is present in the context
            continue;
        }

        // Variable is undefined
        match used_name.as_str() {
            APP_PORT_VAR | APP_PORTS_VAR => return Some((used_name, "this variable is only available when the configuration's `network.reserve-ports` equals `1` or higher. Did you forget to set `network.reserve-ports` in your config file?".to_string())),
            _ => continue,
        };
    }

    None
}

// Variable names
pub const APP_PORT_VAR: &str = "vade.app.network.port";
pub const APP_PORTS_VAR: &str = "vade.app.network.ports";

// Caddyfile templates
pub static CADDYFILE_STATIC_FILES: &str =
    include_str!("resources/caddyfile-templates/static-files.j2");
pub static CADDYFILE_REVERSE_PROXY: &str =
    include_str!("resources/caddyfile-templates/reverse-proxy.j2");

// Systemd unit templates
pub static SYSTEMD_WEBAPP_SERVICE: &str =
    include_str!("resources/systemd-unit-templates/webapp.service.j2");

// Building blocks
static HEADER_TEMPLATE: &str = include_str!("resources/pyinfra-templates/header.py.j2");
static SETUP_TASKS_TEMPLATE: &str = include_str!("resources/pyinfra-templates/create-tasks.py.j2");
static DEPLOY_TASKS_TEMPLATE: &str = include_str!("resources/pyinfra-templates/deploy-tasks.py.j2");
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
    use crate::util::ResolvedPath;
    use std::str::FromStr;

    fn test_render_registered(app_deployment: Option<&AppDeployment>, template: &str) {
        let context = base_minijinja_context(
            Path::new("/tmp/vadegen"),
            Some(&AppName::from_str("foo").unwrap()),
            app_deployment,
        );
        let env = base_minijinja_env().unwrap();
        let template = env.get_template(template).unwrap();
        template.render(context).unwrap();
    }

    #[test]
    fn test_render_deploy_tasks_minimal() {
        let deployment = AppDeployment {
            artifacts: None,
            caddyfile: None,
            systemd_units: vec![],
            reserved_ports: 0,
        };
        test_render_registered(Some(&deployment), "deploy-tasks.py.j2");
    }

    #[test]
    fn test_render_deploy_tasks_full() {
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
            reserved_ports: 0,
        };
        test_render_registered(Some(&deployment), "deploy-tasks.py.j2");
    }

    #[test]
    fn test_render_create() {
        test_render_registered(None, "create-tasks.py.j2");
    }
}
