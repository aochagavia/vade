use crate::application_name::ApplicationName;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "vade", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Set up the necessary files on the server for vade to work properly
    ///
    /// You only need to set up the server once, unless there are vade updates that explicitly
    /// require a new run
    ServerSetup(ServerSetupCommand),
    /// Deploy an application to the server
    ///
    /// If the application hasn't been created yet, this command will create it
    Deploy(DeployCommand),
    /// Create an application on the server
    ///
    /// Prefer the `deploy` command, since it creates the application upon need. This alternative
    /// command is mostly useful when you need to do some initialization work before the first
    /// deployment (e.g., setting up secrets).
    Create(CreateCommand),
}

#[derive(Parser)]
pub struct ServerSetupCommand {
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vadegen")]
    pub out_dir: PathBuf,
}

#[derive(Parser)]
pub struct CreateCommand {
    /// The application's name
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    pub application_name: ApplicationName,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vadegen")]
    pub out_dir: PathBuf,
}

#[derive(Parser)]
pub struct DeployCommand {
    /// The application's name
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    pub application_name: ApplicationName,
    /// The path to your project's configuration file (defaults to `vade.toml`)
    #[arg(short, long = "config")]
    pub configuration_file: Option<PathBuf>,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vadegen")]
    pub out_dir: PathBuf,
}
