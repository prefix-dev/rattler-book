// ~/~ begin <<book/src/ch08-shell-hook.md#src/environment.rs>>[init]
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-imports>>[init]
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};

use crate::project::Project;
// ~/~ end
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-struct>>[init]
/// An installed conda environment that can be activated.
pub struct Environment {
    pub prefix: PathBuf,
    #[allow(dead_code)]
    pub platform: Platform,
}
// ~/~ end
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-impl>>[init]
#[allow(dead_code)]
impl Environment {
    /// Create an environment from a project, with an optional prefix override.
    pub fn from_project(
        project: &Project,
        prefix_override: Option<PathBuf>,
    ) -> miette::Result<Self> {
        let prefix = prefix_override.unwrap_or_else(|| project.default_prefix());
        let prefix = std::path::absolute(prefix).into_diagnostic()?;
        Ok(Self {
            prefix,
            platform: Platform::current(),
        })
    }

    /// Create an environment pointing at an arbitrary prefix.
    pub fn with_prefix(prefix: PathBuf) -> miette::Result<Self> {
        let prefix = std::path::absolute(prefix).into_diagnostic()?;
        Ok(Self {
            prefix,
            platform: Platform::current(),
        })
    }
// ~/~ end
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-impl>>[1]
    /// Bail if the prefix directory does not exist.
    pub fn ensure_exists(&self) -> miette::Result<()> {
        if !self.prefix.exists() {
            miette::bail!(
                "Environment not found at `{}`. Run `shot install` first.",
                self.prefix.display()
            );
        }
        Ok(())
    }
// ~/~ end
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-impl>>[2]
    /// Generate the shell activation script as a string.
    pub fn activate_script(&self, shell_name: Option<&str>) -> miette::Result<String> {
        let shell = parse_shell(shell_name)?;
        let activator =
            Activator::from_path(&self.prefix, shell, self.platform).into_diagnostic()?;
        let vars = ActivationVariables::from_env().into_diagnostic()?;
        let result = activator.activation(vars).into_diagnostic()?;
        result.script.contents().into_diagnostic()
    }
}
// ~/~ end
// ~/~ begin <<book/src/ch08-shell-hook.md#environment-parse-shell>>[init]
fn parse_shell(name: Option<&str>) -> miette::Result<ShellEnum> {
    match name {
        Some(n) => ShellEnum::from_str(n)
            .map_err(|_| miette::miette!("Unknown shell `{n}`. Try: bash, zsh, fish")),
        None => Ok(ShellEnum::from_env().unwrap_or_else(|| Bash.into())),
    }
}
// ~/~ end
// ~/~ end
// ~/~ begin <<book/src/ch09-run.md#src/environment.rs>>[0]
// ~/~ begin <<book/src/ch09-run.md#environment-activation-env>>[init]
impl Environment {
    /// Compute the full set of environment variables that activation
    /// would produce.
    pub async fn activation_env(&self) -> miette::Result<HashMap<String, String>> {
        let prefix = self.prefix.clone();
        let platform = self.platform;

        tokio::task::spawn_blocking(move || {
            let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());
            let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
            let vars = ActivationVariables::from_env().into_diagnostic()?;
            activator.run_activation(vars, None).into_diagnostic()
        })
        .await
        .into_diagnostic()?
    }
}
// ~/~ end
// ~/~ end
