use crate::app_name::AppName;
use crate::cli::{OverrideScope, VarOverride};
use crate::config::AppConfig;
use crate::config::TemplateAndUserVars;
use crate::templating::TomlSource;
use crate::util::{RelativePathResolver, ResolvedPath, diagnostic_with_help};
use miette::{Report, miette};

pub struct AppDeployment {
    pub artifacts: Option<ResolvedPath>,
    pub caddyfile: Option<TemplateAndUserVars>,
    pub systemd_units: Vec<SystemdUnit>,
}

pub struct SystemdUnit {
    pub name: String,
    pub template: TemplateAndUserVars,
}

impl AppDeployment {
    pub fn from_config(
        app_name: &AppName,
        config: AppConfig,
        config_source: &TomlSource,
        overrides: &[VarOverride],
        path_resolver: &RelativePathResolver,
    ) -> Result<Self, Report> {
        let artifacts_dir = config
            .artifacts()
            .map(|artifacts| (artifacts, path_resolver.resolve(&artifacts.path.value)));

        // Sanity check artifacts dir
        if let Some((raw_artifacts, artifacts_dir)) = &artifacts_dir
            && !artifacts_dir.is_dir()
        {
            return Err(diagnostic_with_help(
                "failed to locate artifacts",
                "the provided path does not exist or is not a directory".to_string(),
                format!(
                    "the artifacts path resolved to `{}`",
                    artifacts_dir.display()
                ),
                raw_artifacts.span().into(),
                config_source.to_named_source(),
            ));
        }

        // Load Caddyfile
        let mut caddyfile = config
            .caddyfile()
            .map(|c| c.load_template(config_source, path_resolver))
            .transpose()?;

        // Load systemd units
        let mut systemd_units = Vec::new();
        for c in config.systemd_units() {
            let template = c.load_template(config_source, path_resolver)?;
            systemd_units.push(SystemdUnit {
                name: c.filename(app_name.as_str()),
                template,
            });
        }

        // Apply overrides on top of the user-provided variables from the config
        apply_overrides(overrides, caddyfile.as_mut(), &mut systemd_units)?;

        Ok(AppDeployment {
            artifacts: artifacts_dir.map(|(_, a)| a),
            caddyfile,
            systemd_units,
        })
    }
}

fn apply_overrides(
    overrides: &[VarOverride],
    mut caddyfile: Option<&mut TemplateAndUserVars>,
    systemd_units: &mut [SystemdUnit],
) -> Result<(), Report> {
    for var_override in overrides {
        let user_vars = match var_override.scope {
            OverrideScope::Caddyfile => {
                &mut caddyfile
                    .as_deref_mut()
                    .ok_or_else(|| {
                        miette!(
                            "--set targets `caddyfile`, but the configuration does not have a `[caddyfile]` section"
                        )
                    })?
                    .user_vars
            }
            OverrideScope::SystemdUnit(index) => {
                let systemd_units_len = systemd_units.len();
                &mut systemd_units
                    .get_mut(index)
                    .ok_or_else(|| {
                        miette!(
                            "--set targets `systemd-unit[{index}]`, but the configuration does not have a systemd unit at that index (the total number of systemd units is {})", systemd_units_len)
                    })?
                    .template
                    .user_vars
            }
        };

        user_vars.set(var_override.name.clone(), var_override.value.clone());
    }

    Ok(())
}
