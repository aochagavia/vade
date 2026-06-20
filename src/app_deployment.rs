use crate::app_name::AppName;
use crate::cli::{OverrideScope, VarOverride};
use crate::config::AppConfig;
use crate::templating::TemplateAndUserVars;
use crate::util::{RelativePathResolver, ResolvedPath};
use miette::{Report, miette};

pub struct AppDeployment {
    pub artifacts: Option<ResolvedPath>,
    pub caddyfile: Option<TemplateAndUserVars>,
    pub systemd_units: Vec<SystemdUnit>,
    pub reserved_ports: u32,
}

pub struct SystemdUnit {
    pub name: String,
    pub template: TemplateAndUserVars,
}

impl AppDeployment {
    pub fn from_config(
        app_name: &AppName,
        config: AppConfig,
        overrides: &[VarOverride],
        path_resolver: &RelativePathResolver,
    ) -> Result<Self, Report> {
        let artifacts_dir = config
            .artifacts
            .as_ref()
            .map(|artifacts| path_resolver.resolve(&artifacts.path));

        // Sanity check artifacts dir
        if let Some(artifacts_dir) = &artifacts_dir
            && !artifacts_dir.is_dir()
        {
            return Err(miette!(
                "the provided artifacts directory does not exist or is not a directory (check the path at `{}`)",
                artifacts_dir.display()
            ));
        }

        // Load Caddyfile
        let mut caddyfile = config
            .caddyfile
            .map(|c| c.load_template(path_resolver))
            .transpose()?;

        // Load systemd units
        let mut systemd_units = Vec::new();
        for c in config.systemd_units {
            let template = c.load_template(path_resolver)?;
            let suffix = c
                .filename_suffix
                .map(|s| format!("-{s}"))
                .unwrap_or_default();
            systemd_units.push(SystemdUnit {
                name: format!("{}{suffix}.{}", app_name.as_str(), c.file_extension),
                template,
            });
        }

        // Apply overrides on top of the user-provided variables from the config
        apply_overrides(overrides, caddyfile.as_mut(), &mut systemd_units)?;

        Ok(AppDeployment {
            artifacts: artifacts_dir,
            caddyfile,
            systemd_units,
            reserved_ports: config.network.reserve_ports,
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
                            "override targets `caddyfile`, but the configuration has no `[caddyfile]` section"
                        )
                    })?
                    .user_vars
            }
            OverrideScope::SystemdUnit(index) => {
                &mut systemd_units
                    .get_mut(index)
                    .ok_or_else(|| {
                        miette!(
                            "override targets `systemd-unit[{index}]`, which doesn't exist")
                    })?
                    .template
                    .user_vars
            }
        };

        user_vars.insert(var_override.name.clone(), var_override.value.clone());
    }

    Ok(())
}
