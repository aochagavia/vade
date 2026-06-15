use crate::application_name::ApplicationName;
use minijinja::{Environment, Template, UndefinedBehavior, context};
use rootcause::{Report, bail};
use serde::de::Error;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct TemplateAndExtraVars {
    pub template: String,
    pub user_vars: HashMap<String, minijinja::Value>,
    pub system_vars: HashMap<String, minijinja::Value>,
}

pub fn base_minijinja_context(
    app_meta: Option<&ApplicationMetadata>,
    has_artifacts: bool,
    has_caddyfile: bool,
    has_systemd_unit: bool,
) -> minijinja::Value {
    let base_context = context!(
        VADE_RESERVE_PORTS_SCRIPT => "/opt/vade/scripts/reserve-ports.py",
    );

    let Some(app_meta) = app_meta else {
        return base_context;
    };

    context!(
        APP_NAME => app_meta.name(),
        APP_USERNAME => app_meta.username(),
        APP_HOME_DIR => app_meta.home_dir(),
        APP_SECRETS_FILE => format!("{}/secrets", app_meta.home_dir()),
        APP_STORAGE_DIR => format!("{}/storage", app_meta.home_dir()),
        APP_SYSTEMD_UNIT_FILE => format!("/etc/systemd/system/{}", app_meta.systemd_unit_name()),
        APP_ACTIVE_DEPLOYMENT_DIR => format!("{}/active-deployment", app_meta.home_dir()),
        APP_ACTIVE_ARTIFACTS_DIR => format!("{}/active-deployment/artifacts", app_meta.home_dir()),
        APP_PREVIOUS_DEPLOYMENT_DIR => format!("{}/previous-deployment", app_meta.home_dir()),
        APP_CANDIDATE_DEPLOYMENT_DIR => format!("{}/candidate-deployment", app_meta.home_dir()),
        VADE_SYSTEM_USER_FILE => format!("/opt/vade/system_users/{}", app_meta.username()),
        VADE_SYSTEMD_UNIT_FILE => format!("/opt/vade/systemd_units/{}", app_meta.systemd_unit_name()),
        APP_HAS_ARTIFACTS => has_artifacts,
        APP_HAS_CADDYFILE => has_caddyfile,
        APP_HAS_SYSTEMD_UNIT => has_systemd_unit,
        ..base_context
    )
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

pub struct ApplicationMetadata {
    name: ApplicationName,
}

impl ApplicationMetadata {
    pub fn new(name: ApplicationName) -> Self {
        Self { name }
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn username(&self) -> &str {
        self.name.as_str()
    }

    fn systemd_unit_name(&self) -> String {
        format!("{}.service", self.name())
    }

    fn home_dir(&self) -> String {
        format!("/opt/vade/apps/{}", self.name)
    }
}

// Variable names
pub const APP_PORT_VAR: &str = "APP_PORT";
pub const APP_PORTS_VAR: &str = "APP_PORTS";

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
