mod application_name;
mod cli;
mod commands;
mod config;
mod templating;

use crate::cli::ServerSetupCommand;
use crate::commands::server_setup;
use clap::Parser;
use cli::{Cli, Command, CreateCommand, DeployCommand};
use commands::{create, deploy};
use rootcause::prelude::ResultExt;
use rootcause::{Report, report};
use std::fs;
use std::path::{Path, PathBuf};
use templating::ApplicationMetadata;

fn main() -> Result<(), Report> {
    let cli = Cli::parse();
    match cli.command {
        Command::ServerSetup(cmd) => server_setup(cmd),
        Command::Create(cmd) => create(cmd),
        Command::Deploy(cmd) => deploy(cmd),
    }
}

fn server_setup(command: ServerSetupCommand) -> Result<(), Report> {
    server_setup::ServerSetup {
        out_dir: command.out_dir,
    }
    .execute()
}

fn create(command: CreateCommand) -> Result<(), Report> {
    create::Create {
        application_meta: ApplicationMetadata::new(command.application_name),
        out_dir: command.out_dir,
    }
    .execute()
}

fn deploy(command: DeployCommand) -> Result<(), Report> {
    let uses_default_config_path = command.configuration_file.is_none();
    let config_path = command.configuration_file.unwrap_or("vade.toml".into());
    let config = config::load(&config_path, uses_default_config_path)?;

    // safety: we know that config_path is a file, hence its path always has a parent
    let config_parent_path = config_path.parent().unwrap();

    // Sanity check artifacts dir
    let artifacts_dir = resolve_relative_to(config_parent_path, &config.artifacts.path);
    if !artifacts_dir.is_dir() {
        return Err(report!(
            "the provided artifacts directory does not exist or is not a directory (check the path at `{}`)",
            artifacts_dir.display()
        ));
    }

    // Load files if available
    let caddyfile = config
        .caddyfile
        .map(|c| c.load_template(config_parent_path, &config.network))
        .transpose()?;
    let systemd_unit = config
        .systemd_unit
        .map(|c| c.load_template(config_parent_path, &config.network))
        .transpose()?;

    deploy::Deploy {
        application_meta: ApplicationMetadata::new(command.application_name),
        artifacts_dir: Some(artifacts_dir),
        systemd_unit,
        caddyfile,
        out_dir: command.out_dir,
        reserve_ports: config.network.reserve_ports,
    }
    .execute()
}

fn resolve_relative_to(main: &Path, maybe_relative: &Path) -> PathBuf {
    if maybe_relative.is_absolute() {
        maybe_relative.to_owned()
    } else {
        main.join(maybe_relative)
    }
}

fn read_file(path: &Path) -> Result<String, Report> {
    Ok(fs::read_to_string(path).context(format!("failed to load file at `{}`", path.display()))?)
}
