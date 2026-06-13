use minijinja::context;
use rootcause::{Report, prelude::ResultExt};
use std::{fs, path, path::PathBuf};

use crate::{
    ApplicationMetadata,
    templating::{self, SETUP_PLAYBOOK_TEMPLATE},
};

pub(crate) struct Setup {
    pub(crate) application_meta: ApplicationMetadata,
    pub(crate) out_dir: PathBuf,
}

impl Setup {
    pub(crate) fn execute(self) -> Result<(), Report> {
        fs::create_dir_all(&self.out_dir).context_with(|| {
            format!(
                "failed to create output directory at `{}`",
                self.out_dir.display()
            )
        })?;

        let out_dir_abs = path::absolute(&self.out_dir).unwrap();
        let context =
            templating::base_minijinja_context(&self.application_meta, false, false, false);
        let context = context! {
            LOCAL_RESERVE_PORTS_SCRIPT => out_dir_abs.join("reserve-ports.py").to_string_lossy(),
            ..context,
        };
        let mut env = templating::base_minijinja_env()?;

        // Write the pyinfra deploy
        // safety: the template is always valid
        let deploy = templating::render(
            &mut env,
            &context,
            "setup.py.j2",
            SETUP_PLAYBOOK_TEMPLATE.into(),
        )
        .unwrap();

        fs::write(self.out_dir.join("setup.py"), deploy)
            .context("failed to write pyinfra deploy")?;

        fs::write(self.out_dir.join("reserve-ports.py"), RESERVE_PORTS_SCRIPT)
            .context("failed to write port-reservation script")?;

        Ok(())
    }
}

pub static RESERVE_PORTS_SCRIPT: &str = include_str!("resources/scripts/reserve-ports.py");
