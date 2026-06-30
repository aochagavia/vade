use crate::app_deployment::AppDeployment;
use crate::app_name::AppName;
use crate::templating;
use crate::templating::DEPLOY_TEMPLATE;
use miette::{IntoDiagnostic, Report, WrapErr};
use minijinja::context;
use std::fs;
use std::path::{self, PathBuf};

pub fn execute(
    app_name: AppName,
    app_deployment: AppDeployment,
    out_dir: PathBuf,
    toml_config: &str, // necessary for diagnostics
) -> Result<(), Report> {
    fs::create_dir_all(&out_dir)
        .into_diagnostic()
        .with_context(|| {
            format!(
                "failed to create output directory at `{}`",
                out_dir.display()
            )
        })?;

    let out_dir_abs = path::absolute(&out_dir).unwrap();
    let context =
        templating::base_minijinja_context(&out_dir_abs, Some(&app_name), Some(&app_deployment));

    let mut env = templating::base_minijinja_env()?;

    // Systemd units
    for mut systemd_unit in app_deployment.systemd_units {
        // Render user vars, which are allowed to use jinja templating syntax
        for string_var in systemd_unit.template.user_vars.strings_mut() {
            let rendered =
                templating::render_user_var_string(&mut env, &context, toml_config, string_var)?;
            string_var.value = rendered;
        }

        let context = context!(
            vars => systemd_unit.template.user_vars.into_minijinja(),
            ..context.clone(),
        );
        let rendered = templating::render(
            &mut env,
            &context,
            "unit_file",
            systemd_unit.template.template.clone().into(),
        )
        .context("failed to render jinja2 template for systemd unit")?;

        fs::write(out_dir.join(&systemd_unit.name), rendered)
            .into_diagnostic()
            .context("failed to write systemd unit")?;
    }

    // Caddyfile
    if let Some(mut caddyfile) = app_deployment.caddyfile {
        // Render user vars, which are allowed to use jinja templating syntax
        for string_var in caddyfile.user_vars.strings_mut() {
            let rendered =
                templating::render_user_var_string(&mut env, &context, toml_config, string_var)?;
            string_var.value = rendered;
        }

        let context = context! {
            vars => caddyfile.user_vars.into_minijinja(),
            ..context.clone(),
        };
        let rendered =
            templating::render(&mut env, &context, "caddyfile", caddyfile.template.into())
                .context("failed to render jinja2 template for Caddyfile")?;

        fs::write(out_dir.join("Caddyfile"), rendered)
            .into_diagnostic()
            .context("failed to write caddyfile")?;
    }

    // Pyinfra deploy
    // safety: the template is always valid
    let deploy =
        templating::render(&mut env, &context, "deploy.py.j2", DEPLOY_TEMPLATE.into()).unwrap();
    fs::write(out_dir.join("execute.py"), deploy)
        .into_diagnostic()
        .context("failed to write pyinfra deploy")?;

    Ok(())
}
