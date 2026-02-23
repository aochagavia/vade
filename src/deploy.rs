use minijinja::context;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use std::fs;
use std::path::{self, PathBuf};

use crate::templating::DEPLOY_PLAYBOOK_TEMPLATE;
use crate::{ApplicationMetadata, templating};

pub struct Deploy {
    pub application_meta: ApplicationMetadata,
    pub artifacts_dir: Option<PathBuf>,
    pub systemd_unit: Option<String>,
    pub caddyfile: Option<String>,
    pub out_dir: PathBuf,
    pub skip_setup: bool,
}

impl Deploy {
    fn get_minijinja_context(&self) -> minijinja::Value {
        let mut context = templating::base_minijinja_context(
            &self.application_meta,
            self.artifacts_dir.is_some(),
            self.caddyfile.is_some(),
            self.systemd_unit.is_some(),
        );

        if self.skip_setup {
            context = context! {
                SKIP_SETUP => true,
                ..context,
            };
        }

        let out_dir_abs = path::absolute(&self.out_dir).unwrap();
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
                templating::render(&mut env, &context, "unit_file", systemd_unit.into())
                    .context("invalid jinja2 template for systemd unit")
            })
            .transpose()?;

        let caddyfile_rendered = self
            .caddyfile
            .map(|caddyfile| {
                templating::render(&mut env, &context, "caddyfile", caddyfile.into())
                    .context("invalid jinja2 template for Caddyfile")
            })
            .transpose()?;

        // Write playbook
        // safety: the template is always valid
        let playbook = templating::render(
            &mut env,
            &context,
            "deploy.yml.j2",
            DEPLOY_PLAYBOOK_TEMPLATE.into(),
        )
        .unwrap();

        fs::write(self.out_dir.join("playbook.yml"), playbook)
            .context("failed to write playbook")?;

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
