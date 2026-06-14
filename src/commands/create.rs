use rootcause::{Report, prelude::ResultExt};
use std::{fs, path::PathBuf};

use crate::templating::ApplicationMetadata;
use crate::templating::{self, CREATE_TEMPLATE};

pub(crate) struct Create {
    pub(crate) application_meta: ApplicationMetadata,
    pub(crate) out_dir: PathBuf,
}

impl Create {
    pub(crate) fn execute(self) -> Result<(), Report> {
        fs::create_dir_all(&self.out_dir).context_with(|| {
            format!(
                "failed to create output directory at `{}`",
                self.out_dir.display()
            )
        })?;

        let context =
            templating::base_minijinja_context(Some(&self.application_meta), false, false, false);
        let mut env = templating::base_minijinja_env()?;

        // Write the pyinfra deploy
        // safety: the template is always valid
        let deploy =
            templating::render(&mut env, &context, "create.py.j2", CREATE_TEMPLATE.into()).unwrap();

        fs::write(self.out_dir.join("execute.py"), deploy)
            .context("failed to write pyinfra deploy")?;

        Ok(())
    }
}
