mod deploy;
mod setup;
mod templating;

use clap::{Parser, Subcommand};
use rootcause::prelude::ResultExt;
use rootcause::{Report, report};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::str::FromStr;

static DEFAULT_CADDYFILE: &str = include_str!("resources/Caddyfile");
static DEFAULT_SYSTEMD_SERVICE: &str = include_str!("resources/default.service.j2");

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
    /// The directory where the ansible playbook and related files should be generated
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
    /// The directory where the ansible playbook and related files should be generated
    #[arg(short, long, default_value = "vade-gen")]
    out_dir: PathBuf,
    /// If true, skips setup tasks in the generated playbook
    ///
    /// Offers slightly better speed when deploying new versions of an application
    #[arg(long)]
    skip_setup: bool,
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

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct ApplicationConfig {
    /// The relative path to the directory where the to-be-deployed artifacts are located (if any)
    ///
    /// Note: this path is relative to the configuration file, not to the current working directory
    artifacts_dir: Option<PathBuf>,
    /// The relative path to the systemd unit file that this application should use for deployment
    /// (if any)
    ///
    /// The file itself is templated with `minijinja`, so the relevant paths are injected by the
    /// CLI. That way, the default systemd unit can be reused without changes in most cases.
    ///
    /// Note: this path is relative to the configuration file, not to the current working directory
    systemd_unit_path: Option<PathBuf>,
    /// The relative path to the Caddyfile that this application should use for deployment (if any)
    ///
    /// Note: this path is relative to the configuration file, not to the current working directory
    caddyfile_path: Option<PathBuf>,
}

fn example_app_config() -> ApplicationConfig {
    ApplicationConfig {
        artifacts_dir: Some("artifacts".into()),
        systemd_unit_path: Some("infra/app.service.j2".into()),
        caddyfile_path: Some("infra/Caddyfile".into()),
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
    // Default Caddyfile and systemd units
    fs::create_dir_all("infra").context("failed to create `infra` directory")?;
    fs::write("infra/Caddyfile", DEFAULT_CADDYFILE)
        .context("failed to create default Caddyfile")?;
    fs::write("infra/app.service.j2", DEFAULT_SYSTEMD_SERVICE)
        .context("failed to create default systemd unit")?;

    // Default config (vade.json)
    let default_config = example_app_config();
    let config_file =
        File::create("vade.json").context("failed to create `vade.json`")?;
    serde_json::to_writer_pretty(config_file, &default_config)
        .context("failed to write `vade.json`")?;

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
    let config_path = command
        .configuration_file
        .unwrap_or("vade.json".into());
    let config_json = fs::read(&config_path).context_with(|| {
        let mut msg = format!("failed to load configuration file at `{}`", config_path.display());
        if uses_default_config_path {
            msg.push_str("\n\nno custom path was provided, so the default path was used... did you forget to specify a custom path?");
        }

        msg
    })?;

    let config: ApplicationConfig =
        serde_json::from_slice(&config_json).context("invalid application configuration")?;

    // safety: we know that config_path is a file, hence its path always has a parent
    let config_parent_path = config_path.parent().unwrap();

    // Sanity check artifacts dir
    let artifacts_dir = config
        .artifacts_dir
        .map(|d| path_relative_to(config_parent_path, d));
    if let Some(artifacts_dir) = &artifacts_dir
        && !artifacts_dir.is_dir()
    {
        return Err(report!(
            "the provided artifacts directory does not exist or is not a directory (check the path at `{}`)",
            artifacts_dir.display()
        ));
    }

    // Load files if available
    let caddyfile = match config.caddyfile_path {
        None => None,
        Some(path) => {
            let caddyfile_path = path_relative_to(config_parent_path, path);
            Some(read_file(&caddyfile_path)?)
        }
    };

    let systemd_unit = match config.systemd_unit_path {
        None => None,
        Some(path) => {
            let systemd_unit_path = path_relative_to(config_parent_path, path);
            Some(read_file(&systemd_unit_path)?)
        }
    };

    let deploy = deploy::Deploy {
        application_meta: ApplicationMetadata {
            name: command.application_name,
        },
        artifacts_dir,
        systemd_unit,
        caddyfile,
        out_dir: command.out_dir,
        skip_setup: command.skip_setup,
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
