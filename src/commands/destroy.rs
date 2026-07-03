use crate::app_name::AppName;
use crate::commands::create_out_dir;
use crate::templating::{self, DESTROY_TEMPLATE};
use miette::{IntoDiagnostic, Report, WrapErr};
use std::fs;
use std::path::Path;

pub fn execute(app_name: &AppName, out_dir: &Path) -> Result<(), Report> {
    create_out_dir(out_dir)?;

    let context = templating::base_minijinja_context(out_dir, Some(app_name), None);
    let mut env = templating::base_minijinja_env()?;

    // Write the pyinfra deploy
    let deploy = templating::render_internal(&mut env, &context, "destroy", DESTROY_TEMPLATE)?;

    fs::write(out_dir.join("execute.py"), deploy)
        .into_diagnostic()
        .context("failed to write pyinfra deploy")?;

    Ok(())
}
