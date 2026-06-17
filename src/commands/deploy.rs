use crate::app_deployment::AppDeployment;
use crate::app_name::AppName;
use crate::templating;
use crate::templating::DEPLOY_TEMPLATE;
use minijinja::context;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use std::fs;
use std::path::{self, PathBuf};

pub struct Deploy {
    pub app_name: AppName,
    pub app_deployment: AppDeployment,
    pub out_dir: PathBuf,
}

impl Deploy {
    fn get_minijinja_context(&self) -> minijinja::Value {
        let out_dir_abs = path::absolute(&self.out_dir).unwrap();
        templating::base_minijinja_context(
            &out_dir_abs,
            Some(&self.app_name),
            Some(&self.app_deployment),
        )
    }

    pub fn execute(self) -> Result<(), Report> {
        fs::create_dir_all(&self.out_dir).context_with(|| {
            format!(
                "failed to create output directory at `{}`",
                self.out_dir.display()
            )
        })?;

        let context = self.get_minijinja_context();
        let mut env = templating::base_minijinja_env()?;

        // Systemd units
        for systemd_unit in &self.app_deployment.systemd_units {
            let context = context!(
                vars => systemd_unit.template.user_vars,
                ..context.clone(),
            );
            let rendered = templating::render(
                &mut env,
                &context,
                "unit_file",
                systemd_unit.template.template.clone().into(),
            )
            .context("invalid jinja2 template for systemd unit")?;

            fs::write(self.out_dir.join(&systemd_unit.name), rendered)
                .context("failed to write systemd unit")?;
        }

        // Caddyfile
        if let Some(caddyfile) = self.app_deployment.caddyfile {
            let context = context! {
                vars => caddyfile.user_vars,
                ..context.clone(),
            };
            let rendered =
                templating::render(&mut env, &context, "caddyfile", caddyfile.template.into())
                    .context("invalid jinja2 template for Caddyfile")?;

            fs::write(self.out_dir.join("Caddyfile"), rendered)
                .context("failed to write caddyfile")?;
        }

        // Pyinfra deploy
        // safety: the template is always valid
        let deploy =
            templating::render(&mut env, &context, "deploy.py.j2", DEPLOY_TEMPLATE.into()).unwrap();
        fs::write(self.out_dir.join("execute.py"), deploy)
            .context("failed to write pyinfra deploy")?;

        Ok(())
    }
}
