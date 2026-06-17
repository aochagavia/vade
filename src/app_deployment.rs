use crate::app_name::AppName;
use crate::config::AppConfig;
use crate::templating::TemplateAndUserVars;
use crate::util::{RelativePathResolver, ResolvedPath};
use rootcause::{Report, report};

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
            return Err(report!(
                "the provided artifacts directory does not exist or is not a directory (check the path at `{}`)",
                artifacts_dir.display()
            ));
        }

        // Load Caddyfile
        let caddyfile = config
            .caddyfile
            .map(|c| c.load_template(path_resolver))
            .transpose()?;

        // Load systemd units
        let mut systemd_units = Vec::new();
        for c in config.systemd_units {
            let template = c.load_template(path_resolver)?;
            let suffix = c.file_suffix.map(|s| format!("-{s}")).unwrap_or_default();
            systemd_units.push(SystemdUnit {
                name: format!("{}{suffix}.{}", app_name.as_str(), c.file_extension),
                template,
            });
        }

        Ok(AppDeployment {
            artifacts: artifacts_dir,
            caddyfile,
            systemd_units,
            reserved_ports: config.network.reserve_ports,
        })
    }
}
