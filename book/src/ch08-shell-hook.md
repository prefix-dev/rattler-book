# Chapter 8: The `shell-hook` Command

<span class="newthought">After installing</span> packages into `.env/`, you need to be able to *use* them.
That means getting `lua`, `luarocks`, and any other installed binaries onto
your `PATH`, and setting any environment variables that packages declare.

## Design

```bash
eval $(shot shell-hook)         # bash / zsh
shot shell-hook | source        # fish
```

`$()` runs `shot shell-hook` and captures its text output. `eval` then executes that text as shell commands in the current session. This is how shell-hook can modify your current shell's PATH and other variables.

You can pass an optional `--shell` flag to override the detected shell
dialect and `--prefix` to override the environment location.

/// margin-note
`shot shell-hook` and `shot run` ([Chapter 9](ch09-run.md)) represent a design fork.
`shell-hook` generates a script the user evaluates, which means it must know the
user's shell dialect. `run` spawns a child process with the right
environment variables, which is shell-agnostic but only lasts for one
command. Most package managers end up needing both.
///

## Concepts

### Why activation is necessary

A lot of programs read data from environment variables. Beyond just `PATH`, packages may need `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`, compiler flags, and other variables to function correctly. Activation is the mechanism that sets all of these up.

Different package managers handle this in different ways, some examples are:

1. **conda**: a shell function `eval`s generated shell code.
    - Prepends `bin/` to `PATH`
    - Sets `CONDA_PREFIX` / `CONDA_SHLVL`
    - Sources any scripts packages ship in `activate.d/`
2. **pixi**: conda-compatible activation with three entry points.
    - `pixi run`: prepends `bin/` to `PATH`, sets `CONDA_PREFIX` and `PIXI_*` variables, runs the full activation sequence including `activate.d/` scripts
    - `pixi shell-hook`: prints an eval-able script
    - `pixi shell`: starts an activated shell as a subprocess
3. **uv**: lighter-weight, no activation scripts.
    - `uv tool install` symlinks executables into `~/.local/bin`
    - Virtual-environment activation sets `PATH` and `VIRTUAL_ENV`
    - `uv run` sets env vars on the subprocess directly
4. **Nix**: `nix develop` starts a new shell with `PATH` rebuilt entirely from `/nix/store` paths.
    - Sets build variables like `NIX_CFLAGS_COMPILE` and `PKG_CONFIG_PATH`
    - Runs the derivation's `shellHook`

A child process cannot modify the environment of its parent. This is a basic rule of Unix and Windows process isolation: when a program exits, any environment variable changes it made die with it. So when you run `shot shell-hook`, it can't just set
`PATH` for you, it has to print a script that you evaluate in your shell.

### Shell dialects and nesting

For people to have a native experience they probably want to keep using their shell of choice, which is why popular projects often include multiple shell dialects for their scripts. Several things to take into account:

- **Different shells** have different syntax for setting variables, exporting
  them, and sourcing scripts.  Bash uses `export FOO=bar`; fish uses
  `set -gx FOO bar`; PowerShell uses `$env:FOO = "bar"`.
- **Nested activations**: what if you activate environment A, then activate
  environment B inside it?  You want `PATH` to contain B's bins, then A's, then
  the original `PATH`.
- **Packages with activation scripts**: some packages install scripts in
  `etc/conda/activate.d/` that need to be sourced.  A CUDA package might set
  `LD_LIBRARY_PATH`; an OpenBLAS package might set `OPENBLAS_NUM_THREADS`.

[conda] tracks nesting depth with `CONDA_SHLVL` and the current prefix with
`CONDA_PREFIX`.  [rattler] implements the same protocol.

## Implementation

### The `Environment` struct

Both `shot shell-hook` and `shot run` need to work with an installed environment.

Rather than duplicating prefix-handling and activation logic, we extract it
into a dedicated `Environment` struct in `src/environment.rs`:

``` {.rust file=src/environment.rs}
<<environment-imports>>
<<environment-struct>>
<<environment-impl>>
<<environment-parse-shell>>
```

We bring in the activation and shell types from rattler:

``` {.rust #environment-imports}
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};

use crate::project::Project;
```

The struct holds the prefix path and target platform:

``` {.rust #environment-struct}
/// An installed conda environment that can be activated.
pub struct Environment {
    pub prefix: PathBuf,
    #[allow(dead_code)]
    pub platform: Platform,
}
```

`from_project` creates an environment from a discovered project, using the
default `.env/` prefix unless overridden. `with_prefix` is used when the
caller already knows the exact path (e.g. for a temporary build prefix):

``` {.rust #environment-impl}
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
```

Before activating or running commands, we check that `shot install` has
actually created the prefix. This gives a clear error instead of a confusing
"file not found" later:

``` {.rust #environment-impl}
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
```

`activate_script` is the core of `shot shell-hook`. It detects the user's
shell, builds an `Activator` from the prefix, and returns the activation
script as a string that `eval` can execute:

``` {.rust #environment-impl}
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
```

A small helper resolves the shell dialect from an explicit name or the environment:

``` {.rust #environment-parse-shell}
fn parse_shell(name: Option<&str>) -> miette::Result<ShellEnum> {
    match name {
        Some(n) => ShellEnum::from_str(n)
            .map_err(|_| miette::miette!("Unknown shell `{n}`. Try: bash, zsh, fish")),
        None => Ok(ShellEnum::from_env().unwrap_or_else(|| Bash.into())),
    }
}
```

### The shell-hook command

With `Environment` in place, the shell-hook command becomes very thin:

``` {.rust file=src/commands/shell_hook.rs}
use clap::Parser;

use crate::environment::Environment;
use crate::project::Project;

#[derive(Debug, Parser)]
pub struct Args {
    /// Shell dialect to emit.  Auto-detected from $SHELL if not set.
    ///
    /// Supported values: bash, zsh, fish, xonsh, powershell, cmd, nushell
    #[clap(long)]
    pub shell: Option<String>,

    /// Override the prefix path.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

pub fn execute(args: Args) -> miette::Result<()> {
    let project = Project::discover()?;
    let env = Environment::from_project(&project, args.prefix)?;
    env.ensure_exists()?;

    let script = env.activate_script(args.shell.as_deref())?;
    print!("{script}");
    Ok(())
}
```

Notice that `execute` is *not* `async`.  Generating an activation script is
purely synchronous: no network, no disk I/O beyond reading a few small files
in the prefix.

## `ShellEnum`: a type-safe shell dialect

`rattler_shell::shell::ShellEnum` is an enum with a variant for each supported
shell:

```rust
pub enum ShellEnum {
    Bash(Bash),
    Zsh(Zsh),
    Fish(Fish),
    Xonsh(Xonsh),
    PowerShell(PowerShell),
    CmdExe(CmdExe),
    NuShell(NuShell),
}
```

Each variant wraps a unit struct that implements the `Shell` trait.  The trait
defines methods like `set_env_var`, `export_env_var`, `source_script`, etc.  The
`Activator` calls these methods generically; it doesn't need to know which
shell we're generating for.

### Detecting the shell from `$SHELL`

```rust
ShellEnum::from_env().unwrap_or_else(|| Bash.into())
```

`from_env()` reads `$SHELL` (on Unix) or the default shell (on Windows) and
returns `Option<ShellEnum>`.  If the shell isn't recognized, we fall back to
Bash, which has the widest compatibility.

`Bash.into()` converts the `Bash` struct into `ShellEnum::Bash(Bash)`.

## `Activator::from_path`

```rust
let activator = Activator::from_path(&prefix, shell, platform)?;
```

This reads the prefix and discovers:

1. **Paths to prepend to `PATH`**: typically `<prefix>/bin` on Unix,
   `<prefix>/bin` and `<prefix>/Scripts` on Windows.
2. **Activation scripts**: files in `<prefix>/etc/conda/activate.d/` that
   match the current shell's extension (`.sh`, `.fish`, `.bat`, ...).
3. **Extra environment variables**: from `<prefix>/conda-meta/state` and
   `<prefix>/etc/conda/env_vars.d/`.

## `ActivationVariables`

```rust
let vars = ActivationVariables::from_env()?;
```

This reads the *current* shell state:

- `CONDA_PREFIX`, the currently-activated prefix (if any)
- `CONDA_SHLVL`, the nesting depth
- `PATH`, the current PATH

The activator uses these to correctly compute the transition: deactivate the
current environment (if any), then activate the new one.  The resulting script
handles both the "no active env" case and the "replacing an existing env" case.

## What the generated script looks like

For Bash, `shot shell-hook` might print something like:

```bash
export PATH="/home/user/my-app/.env/bin:${PATH}"
export CONDA_SHLVL=1
export CONDA_ENV_SHLVL_1_CONDA_PREFIX=''
export CONDA_PREFIX=/home/user/my-app/.env
```

You evaluate this, and from that point on `lua`, `luarocks`, etc. are on your
PATH.


## Exercises

!!! exercise-easy "Show Activation Environment Variables"

    Add a `--show-env` flag to `shot shell-hook` that prints the environment variables activation would set, instead of the activation script. Use `Environment::activation_env()` and compare against `std::env::vars()` to show only changed variables.

    /// margin-note
    Call `activation_env()` and compare with the current environment to find changed variables. Note that `activation_env` is async, so `execute` will need to become async too.
    ///

    Acceptance criteria
    :   - `shot shell-hook --show-env` prints lines like `PATH=/path/to/env/bin:...`
        - Only variables that differ from the current environment are shown
        - Variables are sorted alphabetically
        - Count of modified variables printed at the end

!!! exercise-intermediate "Generate Dotenv File from Activation"

    Add `shot shell-hook --dotenv [path]` that writes the activation environment to a dotenv file. This lets other tools (Docker, systemd, IDE run configs) consume the environment without shell-specific activation. Use the `Activator` to compute the full environment, diff against the current env, and write only the changed variables.

    /// margin-note
    Same async `activation_env()` as the previous exercise. Diff against the current env and write only changed variables in dotenv format. Remember to update `main.rs` to `.await` the now-async `execute`.
    ///

    Acceptance criteria
    :   - `shot shell-hook --dotenv` writes `moonshot.env` in the project root (not `.env`, which is the conda prefix directory)
        - `shot shell-hook --dotenv /tmp/my.env` writes to the specified path
        - File format: `KEY=VALUE` per line, values quoted if they contain spaces
        - Only activation-added/changed variables are included (not the full inherited environment)

!!! exercise-hard "Stacked Environment Activation"

    Implement `shot shell-hook --stack /other/env` that generates an activation script layering a second environment on top of the currently active one. Construct `ActivationVariables` from the already-activated environment state, then run the `Activator` for the stacked prefix. The result should have both envs on PATH in the correct order.

    /// margin-note
    Build `ActivationVariables` with `conda_prefix: None` and the base env's paths in the `path` vec. (Setting `conda_prefix: Some(...)` would deactivate the base env instead.) Use `prefix_path_entries` from `rattler_shell::activation` to get the base paths, then `Activator::from_path` and `activator.activation(vars)` for the stacked script. Set `PathModificationBehavior::Prepend`.
    ///

    Acceptance criteria
    :   - `eval $(shot shell-hook)` then `eval $(shot shell-hook --stack /other/env)` puts both envs on PATH
        - Stacked env's `bin/` appears before the base env's `bin/`
        - `CONDA_PREFIX` reflects the top-of-stack environment
        - A `MOONSHOT_STACK_DEPTH` env var tracks nesting level

## Summary

- Shell activation generates a script that the user evaluates to modify their
  shell's environment.
- [rattler_shell] handles multi-shell compatibility (Bash, Fish, PowerShell, ...).
- `Activator::from_path` reads activation metadata from the prefix.
- `ActivationVariables` captures current state for correct nested-activation
  handling.

In the next chapter we implement `shot run`, a way to run a command inside the
activated environment without permanently modifying the shell.

[rattler_shell]: https://crates.io/crates/rattler_shell
[rattler]: https://github.com/conda/rattler
[pixi]: https://pixi.sh
[conda]: https://docs.conda.io
