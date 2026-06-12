use rootcause::{Report, prelude::ResultExt};
use std::{fs, path::PathBuf};

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

        let context =
            templating::base_minijinja_context(&self.application_meta, false, false, false);
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

        Ok(())
    }
}
