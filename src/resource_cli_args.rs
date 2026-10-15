//! One typed Clap argument family reused by full and JSONL-minimal binaries.
//! Project resources are passive until explicitly selected AND fingerprint-approved.
use aw::{config, project_resources::Catalog, Error, Result};
use std::path::{Path, PathBuf};

#[derive(clap::Args, Clone, Debug, Default)]
#[command(next_help_heading = "Project resources & trust")]
pub(crate) struct ResourcesArgs {
    /// Load exact previously approved project resources under this root.
    #[arg(long = "resources-project", help_heading = "Project resources & trust")]
    pub project: Option<PathBuf>,
    /// Pin current project resource file SHA-256 hashes to the private user config; does not call a model.
    #[arg(
        long = "resources-trust-project",
        conflicts_with = "revoke_project",
        help_heading = "Project resources & trust"
    )]
    pub trust_project: Option<PathBuf>,
    /// Revoke the previously pinned project resource trust; does not call a model.
    #[arg(
        long = "resources-revoke-project",
        conflicts_with = "trust_project",
        help_heading = "Project resources & trust"
    )]
    pub revoke_project: Option<PathBuf>,
    /// Use one approved named .agents/prompts/NAME.md as a USER prompt, never system authority.
    #[arg(
        long = "resources-template",
        help_heading = "Project resources & trust"
    )]
    pub template: Option<String>,
    /// Named template value KEY=TEXT; repeatable, size and count bounded, no environment interpolation.
    #[arg(long = "resources-var", help_heading = "Project resources & trust")]
    pub vars: Vec<String>,
    /// Load one approved named .agents/themes/NAME.json; never executes scripts or raw ANSI.
    #[arg(long = "resources-theme", help_heading = "Project resources & trust")]
    pub theme: Option<String>,
}
impl ResourcesArgs {
    pub fn action(&self, file: Option<&Path>, cwd: &Path) -> Result<Option<String>> {
        if (self.trust_project.is_some() || self.revoke_project.is_some())
            && (self.project.is_some()
                || self.template.is_some()
                || self.theme.is_some()
                || !self.vars.is_empty())
        {
            return Err(Error::Config(
                "perform project trust/revocation separately from project resource admission"
                    .into(),
            ));
        }
        if let Some(project) = &self.trust_project {
            let catalog = Catalog::scan(project, cwd)?;
            catalog.trust(file)?;
            return Ok(Some(format!(
                "Approved {} exact source digests for {}; no provider or tool grants changed",
                catalog.approved_files.len(),
                catalog.root.display()
            )));
        }
        if let Some(project) = &self.revoke_project {
            Catalog::revoke(project, file)?;
            return Ok(Some(format!(
                "Revoked project resource trust for {}; no provider/tool accounts affected",
                project.display()
            )));
        }
        Ok(None)
    }
    pub fn admit(&self, cfg: &config::FileConfig, cwd: &Path) -> Result<Option<Catalog>> {
        if (self.template.is_some() || self.theme.is_some() || !self.vars.is_empty())
            && self.project.is_none()
        {
            return Err(Error::Denied(
                "named project templates/themes require explicit --resources-project and trust",
            ));
        }
        if !self.vars.is_empty() && self.template.is_none() {
            return Err(Error::Config(
                "--resources-var requires --resources-template".into(),
            ));
        }
        match &self.project {
            Some(project) => {
                let catalog = Catalog::scan(project, cwd)?;
                catalog.verify(cfg)?;
                if let Some(theme) = &self.theme {
                    catalog.theme(theme)?;
                }
                Ok(Some(catalog))
            }
            None => Ok(None),
        }
    }
    pub fn render_prompt(&self, catalog: Option<&Catalog>, prompt: &str) -> Result<String> {
        match &self.template {
            Some(name) => catalog
                .ok_or(Error::Denied("approved project resources not loaded"))?
                .template(name, prompt, &self.vars),
            None => Ok(prompt.to_owned()),
        }
    }
}
