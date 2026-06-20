use crate::app_name::AppName;
use crate::templating::{self, CREATE_TEMPLATE};
use miette::{IntoDiagnostic, Report, WrapErr};
use std::fs;
use std::path::Path;

pub fn execute(app_name: &AppName, out_dir: &Path) -> Result<(), Report> {
    fs::create_dir_all(out_dir)
        .into_diagnostic()
        .with_context(|| {
            format!(
                "failed to create output directory at `{}`",
                out_dir.display()
            )
        })?;

    let context = templating::base_minijinja_context(out_dir, Some(app_name), None);
    let mut env = templating::base_minijinja_env()?;

    // Write the pyinfra deploy
    // safety: the template is always valid
    let deploy =
        templating::render(&mut env, &context, "create.py.j2", CREATE_TEMPLATE.into()).unwrap();

    fs::write(out_dir.join("execute.py"), deploy)
        .into_diagnostic()
        .context("failed to write pyinfra deploy")?;

    Ok(())
}
