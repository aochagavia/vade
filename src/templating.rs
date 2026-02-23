use std::borrow::Cow;

use minijinja::{Environment, UndefinedBehavior, context};
use rootcause::Report;

use crate::ApplicationMetadata;

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
    env.add_template_owned("playbook-header.yml.j2", PLAYBOOK_HEADER_TEMPLATE)?;
    env.add_template_owned("setup-tasks.yml.j2", SETUP_TASKS_TEMPLATE)?;
    env.add_template_owned("deploy-tasks.yml.j2", DEPLOY_TASKS_TEMPLATE)?;
    Ok(env)
}

pub fn render(
    env: &mut Environment,
    context: &minijinja::Value,
    template_name: &'static str,
    template: Cow<'static, str>,
) -> Result<String, Report> {
    env.add_template_owned(template_name, template)?;

    // safety: we just added the template to the environment
    let template = env.get_template(template_name).unwrap();
    match template.render(context) {
        Ok(s) => Ok(s),
        Err(e) => {
            let mut err = &e as &dyn std::error::Error;
            while let Some(next_err) = err.source() {
                eprintln!();
                eprintln!("caused by: {:#}", next_err);
                err = next_err;
            }

            Err(e.into())
        }
    }
}

// Building blocks
static PLAYBOOK_HEADER_TEMPLATE: &str =
    include_str!("resources/ansible-templates/playbook-header.yml.j2");
static SETUP_TASKS_TEMPLATE: &str = include_str!("resources/ansible-templates/setup-tasks.yml.j2");
static DEPLOY_TASKS_TEMPLATE: &str =
    include_str!("resources/ansible-templates/deploy-tasks.yml.j2");
static PROMOTE_SCRIPT_TEMPLATE: &str =
    include_str!("resources/ansible-templates/deploy-promote.sh.j2");

// Full playbooks
pub static DEPLOY_PLAYBOOK_TEMPLATE: &str =
    include_str!("resources/ansible-templates/deploy.yml.j2");
pub static SETUP_PLAYBOOK_TEMPLATE: &str = include_str!("resources/ansible-templates/setup.yml.j2");
