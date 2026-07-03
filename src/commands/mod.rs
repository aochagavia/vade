use miette::{Context, IntoDiagnostic};
use std::fs;
use std::path::Path;

pub mod create;
pub mod deploy;
pub mod destroy;
pub mod server_setup;

fn create_out_dir(out_dir: &Path) -> miette::Result<()> {
    fs::create_dir_all(out_dir)
        .into_diagnostic()
        .with_context(|| {
            format!(
                "failed to create output directory at `{}`",
                out_dir.display()
            )
        })
}
