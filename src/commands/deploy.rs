use minijinja::context;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use std::fs;
use std::path::{self, PathBuf};

use crate::templating;
use crate::templating::{ApplicationMetadata, DEPLOY_TEMPLATE, TemplateAndExtraVars};

pub struct Deploy {
    pub application_meta: ApplicationMetadata,
    pub artifacts_dir: Option<PathBuf>,
    pub systemd_unit: Option<TemplateAndExtraVars>,
    pub caddyfile: Option<TemplateAndExtraVars>,
    pub out_dir: PathBuf,
    pub reserve_ports: u32,
}

impl Deploy {
    fn get_minijinja_context(&self) -> minijinja::Value {
        let mut context = templating::base_minijinja_context(
            Some(&self.application_meta),
            self.artifacts_dir.is_some(),
            self.caddyfile.is_some(),
            self.systemd_unit.is_some(),
        );

        let out_dir_abs = path::absolute(&self.out_dir).unwrap();
        context = context! {
            APP_RESERVE_PORTS => self.reserve_ports,
            LOCAL_RESERVE_PORTS_SCRIPT => out_dir_abs.join("reserve-ports.py").to_string_lossy(),
            ..context,
        };

        if let Some(artifacts_dir) = &self.artifacts_dir {
            context = context! {
                LOCAL_ARTIFACTS_DIR => path::absolute(artifacts_dir).unwrap().to_string_lossy(),
                ..context,
            };
        }

        if self.caddyfile.is_some() {
            context = context! {
                LOCAL_CADDYFILE_PATH => out_dir_abs.join("Caddyfile").to_string_lossy(),
                ..context,
            };
        }

        if self.systemd_unit.is_some() {
            context = context! {
                LOCAL_SYSTEMD_UNIT_PATH => out_dir_abs.join("app.service").to_string_lossy(),
                ..context,
            };
        }

        context
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

        let systemd_unit_rendered = self
            .systemd_unit
            .map(|systemd_unit| {
                let context = context! {
                    ..context.clone(),
                    ..systemd_unit.extra_vars
                };
                templating::render(
                    &mut env,
                    &context,
                    "unit_file",
                    systemd_unit.template.into(),
                )
                .context("invalid jinja2 template for systemd unit")
            })
            .transpose()?;

        let caddyfile_rendered = self
            .caddyfile
            .map(|caddyfile| {
                let context = context! {
                    ..context.clone(),
                    ..caddyfile.extra_vars
                };
                templating::render(&mut env, &context, "caddyfile", caddyfile.template.into())
                    .context("invalid jinja2 template for Caddyfile")
            })
            .transpose()?;

        // Write the pyinfra deploy
        // safety: the template is always valid
        let deploy =
            templating::render(&mut env, &context, "deploy.py.j2", DEPLOY_TEMPLATE.into()).unwrap();

        fs::write(self.out_dir.join("execute.py"), deploy)
            .context("failed to write pyinfra deploy")?;

        if let Some(systemd_unit) = systemd_unit_rendered {
            fs::write(self.out_dir.join("app.service"), systemd_unit)
                .context("failed to write systemd unit")?;
        }

        if let Some(caddyfile) = caddyfile_rendered {
            fs::write(self.out_dir.join("Caddyfile"), caddyfile)
                .context("failed to write caddyfile")?;
        }

        Ok(())
    }
}
