use crate::ApplicationMetadata;
use minijinja::{Environment, UndefinedBehavior, context};
use rootcause::Report;
use serde::de::Error;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

pub struct TemplateAndExtraVars {
    pub template: String,
    pub extra_vars: HashMap<&'static str, minijinja::Value>,
}

pub fn base_minijinja_context(
    app_meta: &ApplicationMetadata,
    has_artifacts: bool,
    has_caddyfile: bool,
    has_systemd_unit: bool,
) -> minijinja::Value {
    context!(
        APP_NAME => app_meta.name(),
        APP_USERNAME => app_meta.username(),
        APP_HOME_DIR => app_meta.home_dir(),
        APP_HAS_ARTIFACTS => has_artifacts,
        APP_HAS_CADDYFILE => has_caddyfile,
        APP_HAS_SYSTEMD_UNIT => has_systemd_unit,
        APP_SECRETS_FILE => format!("{}/secrets", app_meta.home_dir()),
        APP_STORAGE_DIR => format!("{}/storage", app_meta.home_dir()),
        APP_SYSTEMD_UNIT_FILE => format!("/etc/systemd/system/{}", app_meta.systemd_unit_name()),
        APP_ACTIVE_DEPLOYMENT_DIR => format!("{}/active-deployment", app_meta.home_dir()),
        APP_ACTIVE_ARTIFACTS_DIR => format!("{}/active-deployment/artifacts", app_meta.home_dir()),
        APP_PREVIOUS_DEPLOYMENT_DIR => format!("{}/previous-deployment", app_meta.home_dir()),
        APP_CANDIDATE_DEPLOYMENT_DIR => format!("{}/candidate-deployment", app_meta.home_dir()),
        VADE_SYSTEM_USER_FILE => format!("/opt/vade/system_users/{}", app_meta.username()),
        VADE_SYSTEMD_UNIT_FILE => format!("/opt/vade/systemd_units/{}", app_meta.systemd_unit_name()),
    )
}

pub fn base_minijinja_env() -> Result<Environment<'static>, Report> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.add_template_owned("deploy-promote.sh.j2", PROMOTE_SCRIPT_TEMPLATE)?;
    env.add_template_owned("header.py.j2", HEADER_TEMPLATE)?;
    env.add_template_owned("setup-tasks.py.j2", SETUP_TASKS_TEMPLATE)?;
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
    // We re-render the results of the previous render until we reach a fixpoint. This is to allow
    // for using variables inside other jinja variables.
    let mut template_string = template.to_string();
    let mut i = 0;
    loop {
        let template_name = format!("{template_name}{i}");
        i += 1;
        env.add_template_owned(template_name.clone(), template_string.clone())?;

        // safety: we just added the template to the environment
        let template = env.get_template(&template_name).unwrap();

        let rendered = match template.render(context) {
            Ok(s) => s,
            Err(e) => {
                let mut err = &e as &dyn std::error::Error;
                while let Some(next_err) = err.source() {
                    eprintln!();
                    eprintln!("caused by: {:#}", next_err);
                    err = next_err;
                }

                return Err(e.into());
            }
        };

        if template_string == rendered {
            return Ok(rendered);
        } else {
            template_string = rendered;
        }
    }
}

// Caddyfile templates
pub static CADDYFILE_STATIC_FILES: &str =
    include_str!("resources/caddyfile-templates/static-files.j2");
pub static CADDYFILE_REVERSE_PROXY: &str =
    include_str!("resources/caddyfile-templates/reverse-proxy.j2");

// Systemd unit templates
pub static SYSTEMD_APPLICATION: &str =
    include_str!("resources/systemd-unit-templates/application.service.j2");

// Building blocks
static HEADER_TEMPLATE: &str = include_str!("resources/pyinfra-templates/header.py.j2");
static SETUP_TASKS_TEMPLATE: &str = include_str!("resources/pyinfra-templates/setup-tasks.py.j2");
static DEPLOY_TASKS_TEMPLATE: &str = include_str!("resources/pyinfra-templates/deploy-tasks.py.j2");
static PROMOTE_SCRIPT_TEMPLATE: &str =
    include_str!("resources/pyinfra-templates/deploy-promote.sh.j2");

// Full deploys
pub static DEPLOY_PLAYBOOK_TEMPLATE: &str =
    include_str!("resources/pyinfra-templates/deploy.py.j2");
pub static SETUP_PLAYBOOK_TEMPLATE: &str = include_str!("resources/pyinfra-templates/setup.py.j2");
