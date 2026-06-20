mod app_deployment;
mod app_name;
mod cli;
mod commands;
mod config;
mod templating;
mod util;

use crate::cli::ServerSetupCommand;
use crate::commands::server_setup;
use crate::util::RelativePathResolver;
use app_deployment::AppDeployment;
use clap::Parser;
use cli::{Cli, Command, DeployCommand};
use commands::{create, deploy};
use miette::{IntoDiagnostic, Report, WrapErr};
use std::fs;
use std::path::Path;

fn main() -> Result<(), Report> {
    let cli = Cli::parse();
    match cli.command {
        Command::ServerSetup(cmd) => server_setup(cmd),
        Command::Create(cmd) => create::execute(&cmd.app_name, &cmd.out_dir),
        Command::Deploy(cmd) => deploy(cmd),
    }
}

fn server_setup(command: ServerSetupCommand) -> Result<(), Report> {
    server_setup::ServerSetup {
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
    let path_resolver = RelativePathResolver::with_root(config_parent_path.to_owned());

    deploy::Deploy {
        app_deployment: AppDeployment::from_config(
            &command.app_name,
            config,
            &command.set_json,
            &path_resolver,
        )?,
        app_name: command.app_name,
        out_dir: command.out_dir,
    }
    .execute()
}

fn read_file(path: &Path) -> Result<String, Report> {
    fs::read_to_string(path)
        .into_diagnostic()
        .context(format!("failed to load file at `{}`", path.display()))
}
