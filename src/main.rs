mod config;
mod deploy;
mod setup;
mod templating;

use clap::{Parser, Subcommand};
use rootcause::prelude::ResultExt;
use rootcause::{Report, report};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "vade", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    Deploy(DeployCommand),
    Setup(SetupCommand),
}

#[derive(Parser)]
struct SetupCommand {
    /// The application's name
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    application_name: ApplicationName,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vade-gen")]
    out_dir: PathBuf,
}

#[derive(Parser)]
struct DeployCommand {
    /// The application's name
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    application_name: ApplicationName,
    /// The path to vanilla deploy's configuration file (defaults to `vade.json`)
    #[arg(short, long = "config")]
    configuration_file: Option<PathBuf>,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vade-gen")]
    out_dir: PathBuf,
}

#[derive(Clone)]
struct ApplicationName {
    inner: String,
}

impl ApplicationName {
    fn as_str(&self) -> &str {
        &self.inner
    }
}

impl Display for ApplicationName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl FromStr for ApplicationName {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.is_ascii() {
            return Err("only ASCII is allowed inside application names");
        }

        let valid = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            return Err(
                "only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed",
            );
        }

        Ok(Self {
            inner: s.to_string(),
        })
    }
}

struct ApplicationMetadata {
    name: ApplicationName,
}

impl ApplicationMetadata {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn username(&self) -> &str {
        self.name.as_str()
    }

    fn systemd_unit_name(&self) -> String {
        format!("{}.service", self.name())
    }

    fn home_dir(&self) -> String {
        format!("/opt/vade/apps/{}", self.name)
    }
}

fn main() -> Result<(), Report> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => init(),
        Command::Setup(cmd) => setup(cmd),
        Command::Deploy(cmd) => deploy(cmd),
    }
}

fn init() -> Result<(), Report> {
    // TODO: rework this
    // // Default Caddyfile and systemd units
    // fs::create_dir_all("infra").context("failed to create `infra` directory")?;
    // fs::write("infra/Caddyfile", DEFAULT_CADDYFILE)
    //     .context("failed to create default Caddyfile")?;
    // fs::write("infra/app.service.j2", DEFAULT_SYSTEMD_SERVICE)
    //     .context("failed to create default systemd unit")?;
    //
    // // Default config (vade.json)
    // let default_config = config::example_app_config();
    // let config_file = File::create("vade.json").context("failed to create `vade.json`")?;
    // serde_json::to_writer_pretty(config_file, &default_config)
    //     .context("failed to write `vade.json`")?;

    Ok(())
}

fn setup(command: SetupCommand) -> Result<(), Report> {
    let setup = setup::Setup {
        application_meta: ApplicationMetadata {
            name: command.application_name,
        },
        out_dir: command.out_dir,
    };

    setup.execute()
}

fn deploy(command: DeployCommand) -> Result<(), Report> {
    let uses_default_config_path = command.configuration_file.is_none();
    let config_path = command.configuration_file.unwrap_or("vade.toml".into());
    let config = config::load(&config_path, uses_default_config_path)?;

    // safety: we know that config_path is a file, hence its path always has a parent
    let config_parent_path = config_path.parent().unwrap();

    // Sanity check artifacts dir
    let artifacts_dir = path_relative_to(config_parent_path, config.artifacts.path);
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

    let deploy = deploy::Deploy {
        application_meta: ApplicationMetadata {
            name: command.application_name,
        },
        artifacts_dir: Some(artifacts_dir),
        systemd_unit,
        caddyfile,
        out_dir: command.out_dir,
        reserve_ports: config.network.reserve_ports,
    };

    deploy.execute()
}

fn path_relative_to(main: &Path, maybe_relative: PathBuf) -> PathBuf {
    if maybe_relative.is_absolute() {
        maybe_relative
    } else {
        main.join(maybe_relative)
    }
}

fn read_file(path: &Path) -> Result<String, Report> {
    Ok(fs::read_to_string(path).context(format!("failed to load file at `{}`", path.display()))?)
}
