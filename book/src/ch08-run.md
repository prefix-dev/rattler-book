# Chapter 8: The `run` Command

`luapkg shell` requires the user to evaluate shell-specific output. That works for interactive use but creates two problems: it is awkward to use in scripts and CI pipelines, and it ties you to a specific shell dialect. `luapkg run` solves both. It computes the activated environment internally and spawns the command as a child process, so it works the same way regardless of whether the user runs Bash, Fish, PowerShell, or no shell at all.

## Design

```bash
luapkg run lua -e 'print("hello from conda")'
luapkg run luarocks install inspect
```

Everything after `run` is passed verbatim to the OS. The command accepts an
optional `--prefix` flag to override the environment location.

## Concepts

### Environment diffing

We can't modify the parent shell's environment, but we *can* control the
environment of a child process.  The trick is:

1. Compute what the environment *would* look like after activation.
2. Spawn the user's command with that modified environment.

The child inherits the modified environment; the parent is untouched. This is the same pattern pixi uses for `pixi run`.

This uses the same activation logic from [Chapter 7](ch07-shell.md), but instead of printing a script it captures the resulting environment as a map of variable names to values. Because `run_activation` executes the full activation sequence (including any `activate.d` scripts that packages ship), dynamic environment variables like `PKG_CONFIG_PATH` and `LUA_PATH` are picked up automatically.

## Implementation

Here is the complete `src/commands/run.rs`:

``` {.rust file=src/commands/run.rs}
use std::env;
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};
use tokio::process::Command;

#[derive(Debug, Parser)]
pub struct Args {
    /// The command to run (and its arguments).
    ///
    /// Everything after `run` is passed verbatim to the OS.
    #[clap(required = true, trailing_var_arg = true)]
    pub command: Vec<String>,

    /// Override the prefix path.
    #[clap(long)]
    pub prefix: Option<std::path::PathBuf>,
}

pub async fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = args
        .prefix
        .unwrap_or_else(|| super::prefix_dir(&cwd));
    let prefix = std::path::absolute(prefix).into_diagnostic()?;

    if !prefix.exists() {
        miette::bail!(
            "Environment not found at `{}`. Run `luapkg install` first.",
            prefix.display()
        );
    }

    let platform = Platform::current();
    let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());

    let activator =
        Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
    let current_vars = ActivationVariables::from_env().into_diagnostic()?;

    let activation_env = tokio::task::spawn_blocking(move || {
        activator.run_activation(current_vars, None)
    })
    .await
    .into_diagnostic()?
    .into_diagnostic()?;

    let (program, rest_args) = args.command.split_first().expect("clap ensures non-empty");

    let status = Command::new(program)
        .args(rest_args)
        .envs(&activation_env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .into_diagnostic()?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }

    Ok(())
}
```

rattler provides the key function `Activator::run_activation`:

```rust
let activation_env = tokio::task::spawn_blocking(move || {
    activator.run_activation(current_vars, None)
})
.await
.into_diagnostic()? // JoinError
.into_diagnostic()?; // ActivationError
```

`run_activation` works by writing a temporary shell script that:
1. Emits the current environment (via `env` on Unix, `set` on Windows).
2. Sources the activation logic.
3. Emits the environment again.

It then runs that script and diffs the two snapshots, returning only the changed
variables as a `HashMap<String, String>`.

### Spawning the child process

```rust
let (program, rest_args) = args.command.split_first().expect("clap ensures non-empty");

let status = Command::new(program)
    .args(rest_args)
    .envs(&activation_env)
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .status()
    .await
    .into_diagnostic()?;
```

We use `tokio::process::Command` (the async version of `std::process::Command`).

`.envs(&activation_env)` overlays the activation variables on top of the
*inherited* environment.  So the child gets:
- All of the current process's environment variables (PATH, HOME, etc.)
- Plus the activation changes (extended PATH, CONDA_PREFIX, etc.)

`.stdin(Stdio::inherit())` / `.stdout(Stdio::inherit())` / `.stderr(Stdio::inherit())`
connect the child's stdio to the parent's.  The child can read from the terminal
and write to it directly; `lua` works interactively.

`.status()` runs the command and returns its exit status once it completes,
without capturing stdout/stderr.

### Propagating the exit code

!!! warning "Exit code propagation"

    Without exit code propagation, `luapkg run` is unusable in CI: a failing
    test would appear as a successful pipeline step.

```rust
if !status.success() {
    let code = status.code().unwrap_or(1);
    std::process::exit(code);
}
```

If the child fails, we exit with the same code.  This lets `luapkg run` compose
correctly in shell scripts:

```bash
luapkg run lua test.lua || echo "tests failed"
```

`std::process::exit(code)` terminates the process immediately with the given
exit code.  It doesn't run destructors or flush buffers, but since we're about
to exit anyway, that's fine.

Why not `return Err(...)`?  Because there's no meaningful error to report.  The
child ran successfully but indicated failure via its exit code.  Returning an
error would cause `miette` to print an error message, cluttering the output.

## Summary

- `luapkg run` computes a modified environment via `run_activation` and spawns
  a child process with it.
- `spawn_blocking` offloads synchronous, potentially-blocking code to a
  dedicated thread pool.
- `.envs()` overlays activation variables on the inherited environment.
- `.stdin/stdout/stderr(Stdio::inherit())` gives the child full terminal access.
- Exit codes are propagated so `luapkg run` composes in shell scripts.

In the next chapter, the most complex one, we implement `luapkg build`: turning
source code into a distributable conda package.
