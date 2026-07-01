use crate::app_name::AppName;
use crate::config::UserVar;
use clap::{Parser, Subcommand};
use miette::{Report, bail, miette};
use std::path::PathBuf;
use std::str::FromStr;

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
    /// The name of the app to create on the server
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    pub app_name: AppName,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vadegen")]
    pub out_dir: PathBuf,
}

#[derive(Parser)]
pub struct DeployCommand {
    /// The name of the app that this deployment targets on the server
    ///
    /// Only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed
    pub app_name: AppName,
    /// The path to your project's configuration file (defaults to `vade.toml`)
    #[arg(short, long = "config")]
    pub configuration_file: Option<PathBuf>,
    /// The directory where the pyinfra deploy and related files should be generated
    #[arg(short, long, default_value = "vadegen")]
    pub out_dir: PathBuf,
    /// Override a template variable with a JSON value
    ///
    /// Example 1: `--set 'caddyfile.vars.domains=["example.com", "www.example.com"]'`
    ///
    /// Example 2: `--set 'systemd-unit[0].vars.exec_start="touch /tmp/i-was-here"'`
    #[arg(long = "set", value_name = "PATH=JSON")]
    pub set_json: Vec<VarOverride>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OverrideScope {
    Caddyfile,
    SystemdUnit(usize),
}

#[derive(Debug, Clone)]
pub struct VarOverride {
    pub scope: OverrideScope,
    pub name: String,
    pub value: UserVar,
}

impl FromStr for VarOverride {
    type Err = Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (path, raw_value) = s
            .split_once('=')
            .ok_or_else(|| miette!("expected the format `<path>=<value>`"))?;

        let (scope, name) = parse_path(path)?;

        let json: serde_json::Value = serde_json::from_str(raw_value)
            .map_err(|e| miette!("failed to parse JSON in `{raw_value}`, {e}"))?;
        let value = UserVar::from_json(path, json);

        Ok(VarOverride { scope, name, value })
    }
}

fn parse_path(path: &str) -> Result<(OverrideScope, String), Report> {
    let error = miette!(
        "failed to parse path `{path}`: it must start with `caddyfile.vars.` or `systemd-unit[<index>].vars.`"
    );
    if let Some(name) = path.strip_prefix("caddyfile.vars.") {
        if name.is_empty() {
            bail!(error);
        }
        return Ok((OverrideScope::Caddyfile, name.to_owned()));
    }

    if let Some(rest) = path.strip_prefix("systemd-unit[") {
        let Some((index, var_name)) = rest.split_once("].vars.") else {
            bail!(error)
        };
        let index: usize = index.parse().map_err(|e| {
            miette!("failed to parse path `{path}`: `{index}` is not a valid index ({e})")
        })?;
        if var_name.is_empty() {
            bail!(error)
        };
        return Ok((OverrideScope::SystemdUnit(index), var_name.to_string()));
    }

    bail!(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserVarString;

    #[test]
    fn parses_caddyfile_domains() {
        let o = VarOverride::from_str("caddyfile.vars.domains=[\"example.com\"]").unwrap();
        assert_eq!(o.scope, OverrideScope::Caddyfile);
        assert_eq!(o.name, "domains");
        assert_eq!(
            o.value,
            UserVar::List(vec![UserVar::String(UserVarString::json(
                "example.com".to_string(),
                "caddyfile.vars.domains".to_string()
            ))])
        );
    }

    #[test]
    fn parses_systemd_exec_start() {
        let o = VarOverride::from_str(r#"systemd-unit[2].vars.exec_start="touch /tmp/i-was-here""#)
            .unwrap();
        assert_eq!(o.scope, OverrideScope::SystemdUnit(2));
        assert_eq!(o.name, "exec_start");
        let expected = UserVar::String(UserVarString::json(
            "touch /tmp/i-was-here".to_string(),
            "systemd-unit[2].vars.exec_start".to_string(),
        ));
        assert_eq!(o.value, expected);
    }

    #[test]
    fn value_may_contain_equals_sign() {
        let o = VarOverride::from_str(r#"systemd-unit[0].vars.exec_start="run --flag=1""#).unwrap();
        assert_eq!(o.name, "exec_start");
        assert_eq!(
            o.value,
            UserVar::String(UserVarString::json(
                "run --flag=1".to_string(),
                "systemd-unit[0].vars.exec_start".to_string()
            ))
        );
    }

    #[test]
    fn rejects_missing_equals() {
        assert!(VarOverride::from_str("caddyfile.vars.domains").is_err());
    }

    #[test]
    fn rejects_invalid_index() {
        assert!(VarOverride::from_str("systemd-unit[x].vars.foo=1").is_err());
        assert!(VarOverride::from_str("systemd-unit[0.foo=1").is_err());
    }

    #[test]
    fn rejects_empty_variable_name() {
        assert!(VarOverride::from_str("caddyfile.vars.=x").is_err());
        assert!(VarOverride::from_str("systemd-unit[0].vars.=x").is_err());
        assert!(VarOverride::from_str("systemd-unit[0].vars=x").is_err());
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(VarOverride::from_str("caddyfile.vars.domains=[").is_err());
    }
}
