# Chapter 8: The `shell` Command

After installing packages into `.env/`, you need to be able to *use* them.
That means getting `lua`, `luarocks`, and any other installed binaries onto
your `PATH`, and setting any environment variables that packages declare.

## Design

```bash
eval $(shot shell)         # bash / zsh
shot shell | source        # fish
```

The `eval` trick runs the output of `shot shell` as shell code in the current
process, which *can* modify `PATH`.

You can pass an optional `--shell` flag to override the detected shell
dialect and `--prefix` to override the environment location.

!!! info "`shell` vs `run`"

    `shot shell` and `shot run` ([Chapter 9](ch09-run.md)) represent a design fork.
    `shell` generates a script the user evaluates, which means it must know the
    user's shell dialect. `run` spawns a child process with the right
    environment variables, which is shell-agnostic but only lasts for one
    command. Most package managers end up needing both.

## Concepts

### Why activation is non-trivial

Package managers handle activation in different ways. Some use shims (small wrapper executables that redirect to the right version), others use wrapper scripts that set `PATH` before invoking the tool. conda uses eval-based activation: the tool prints a shell script and you evaluate it in your current shell. This gives packages full control over the environment (including `PATH`, `LD_LIBRARY_PATH`, `LUA_PATH`, and other variables), at the cost of being shell-dependent.

A child process cannot modify the environment of its parent.  That's a Unix
rule with no exceptions.  So when you run `shot shell`, it can't just set
`PATH` for you; it has to print a script that you evaluate in your shell.

### Shell dialects and nesting

This sounds simple, but several complications arise:

- **Different shells** have different syntax for setting variables, exporting
  them, and sourcing scripts.  Bash uses `export FOO=bar`; fish uses
  `set -gx FOO bar`; PowerShell uses `$env:FOO = "bar"`.
- **Nested activations**: what if you activate environment A, then activate
  environment B inside it?  You want `PATH` to contain B's bins, then A's, then
  the original `PATH`.
- **Packages with activation scripts**: some packages install scripts in
  `etc/conda/activate.d/` that need to be sourced.  A CUDA package might set
  `LD_LIBRARY_PATH`; an OpenBLAS package might set `OPENBLAS_NUM_THREADS`.

conda tracks nesting depth with `CONDA_SHLVL` and the current prefix with
`CONDA_PREFIX`.  rattler implements the same protocol.

## Implementation

``` {.rust file=src/commands/shell.rs}
use std::env;
use std::str::FromStr;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};

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
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = args.prefix.unwrap_or_else(|| super::prefix_dir(&cwd));
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    if !prefix.exists() {
        miette::bail!(
            "Environment not found at `{}`. Run `shot install` first.",
            prefix.display()
        );
    }

    let platform = Platform::current();

    let shell: ShellEnum = if let Some(ref name) = args.shell {
        ShellEnum::from_str(name)
            .map_err(|_| miette::miette!("Unknown shell `{name}`. Try: bash, zsh, fish"))?
    } else {
        ShellEnum::from_env().unwrap_or_else(|| Bash.into())
    };

    let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;

    let vars = ActivationVariables::from_env().into_diagnostic()?;

    let result = activator.activation(vars).into_diagnostic()?;
    let script = result.script.contents().into_diagnostic()?;

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

For Bash, `shot shell` might print something like:

```bash
export PATH="/home/user/my-app/.env/bin:$PATH"
export CONDA_PREFIX="/home/user/my-app/.env"
export CONDA_SHLVL="1"
export CONDA_DEFAULT_ENV="/home/user/my-app/.env"
# source activation scripts
. "/home/user/my-app/.env/etc/conda/activate.d/lua_path.sh"
```

You evaluate this, and from that point on `lua`, `luarocks`, etc. are on your
PATH.

!!! note "Why `CONDA_` variables?"

    You might notice that a Lua package manager is setting `CONDA_PREFIX` and
    `CONDA_SHLVL`. This is because rattler implements conda's activation
    protocol, and these variables are part of that protocol. The benefit is
    compatibility: any tool that understands conda environments (editors, CI
    systems, other package managers) will recognize our environment. The cost is
    the naming confusion, since "CONDA" has nothing to do with Lua. A more
    polished tool could alias these variables, but you would lose the ecosystem
    compatibility.

## Summary

- Shell activation generates a script that the user evaluates to modify their
  shell's environment.
- `rattler_shell` handles multi-shell compatibility (Bash, Fish, PowerShell, ...).
- `Activator::from_path` reads activation metadata from the prefix.
- `ActivationVariables` captures current state for correct nested-activation
  handling.

In the next chapter we implement `shot run`, a way to run a command inside the
activated environment without permanently modifying the shell.
