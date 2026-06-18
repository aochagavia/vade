use crate::templating;
use crate::templating::SERVER_SETUP_TEMPLATE;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use std::path::PathBuf;
use std::{fs, path};

pub struct ServerSetup {
    pub out_dir: PathBuf,
}

impl ServerSetup {
    pub fn execute(self) -> Result<(), Report> {
        fs::create_dir_all(&self.out_dir).context_with(|| {
            format!(
                "failed to create output directory at `{}`",
                self.out_dir.display()
            )
        })?;

        let out_dir_abs = path::absolute(&self.out_dir).unwrap();
        let context = templating::base_minijinja_context(&out_dir_abs, None, None);
        let mut env = templating::base_minijinja_env()?;

        // Write the pyinfra deploy
        // safety: the template is always valid
        let server_setup = templating::render(
            &mut env,
            &context,
            "server-setup.py.j2",
            SERVER_SETUP_TEMPLATE.into(),
        )
        .unwrap();

        fs::write(self.out_dir.join("execute.py"), server_setup)
            .context("failed to write pyinfra deploy")?;

        fs::write(self.out_dir.join("assign-ports.py"), RESERVE_PORTS_SCRIPT)
            .context("failed to write port-reservation script")?;

        Ok(())
    }
}

pub static RESERVE_PORTS_SCRIPT: &str = include_str!("../resources/scripts/assign-ports.py");
