# Chapter 9: The `run` Command

`shot shell` requires you to evaluate shell-specific output. That works for interactive use but creates two problems: it's awkward in scripts and CI pipelines, and it ties you to a specific shell dialect. `shot run` solves both. It computes the activated environment internally and spawns the command as a child process, so it works the same way regardless of whether you use Bash, Fish, PowerShell, or no shell at all.

## Design

```bash
shot run lua -e 'print("hello from conda")'
shot run luarocks install inspect
```

Everything after `run` is passed verbatim to the OS. You can pass an optional
`--prefix` flag to override the environment location.

## Concepts

### Environment diffing

We can't modify the parent shell's environment, but we *can* control the
environment of a child process.  The trick is:

1. Compute what the environment *would* look like after activation.
2. Spawn the user's command with that modified environment.

The child inherits the modified environment; the parent is untouched. This is the same pattern pixi uses for `pixi run`.

This uses the same activation logic from [Chapter 8](ch08-shell-hook.md), but instead of printing a script it captures the resulting environment as a map of variable names to values. Because `run_activation` executes the full activation sequence (including any `activate.d` scripts that packages ship), dynamic environment variables like `PKG_CONFIG_PATH` and `LUA_PATH` are picked up automatically.

## Implementation

The full `src/commands/run.rs` is assembled from three named sections:

``` {.rust file=src/commands/run.rs}
<<run-imports>>

<<run-args>>

<<run-execute>>
```

### Imports

``` {.rust #run-imports}
use std::env;
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use rattler_conda_types::Platform;
use rattler_shell::activation::{ActivationVariables, Activator};
use rattler_shell::shell::{Bash, ShellEnum};
use tokio::process::Command;
```

### Command arguments

Our `Args` struct accepts a trailing variadic argument for the command to run,
plus an optional `--prefix` override.

``` {.rust #run-args}
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
```

### The execute function

Our `execute` function breaks into four steps: resolve the prefix and build
the activator, compute the activation environment, spawn the child process,
and propagate its exit code.

``` {.rust #run-execute}
pub async fn execute(args: Args) -> miette::Result<()> {
    <<run-setup>>

    <<run-activation>>

    <<run-spawn>>

    <<run-exit-code>>
}
```

First we resolve the prefix path and construct an `Activator`. If the prefix
doesn't exist, we bail early with a helpful message.

``` {.rust #run-setup}
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
let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());

let activator = Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
let current_vars = ActivationVariables::from_env().into_diagnostic()?;
```

rattler provides the key function `Activator::run_activation`.
`run_activation` works by writing a temporary shell script that:

1. Emits the current environment (via `env` on Unix, `set` on Windows).
2. Sources the activation logic.
3. Emits the environment again.

It then runs that script and diffs the two snapshots, returning only the changed
variables as a `HashMap<String, String>`.

``` {.rust #run-activation}
let activation_env =
    tokio::task::spawn_blocking(move || activator.run_activation(current_vars, None))
        .await
        .into_diagnostic()?
        .into_diagnostic()?;
```

### Spawning the child process

We use `tokio::process::Command` (the async version of `std::process::Command`)
to launch the child.

`.envs(&activation_env)` overlays the activation variables on top of the
*inherited* environment.  So the child gets:

- All of the current process's environment variables (PATH, HOME, etc.)
- Plus the activation changes (extended PATH, CONDA_PREFIX, etc.)

`.stdin(Stdio::inherit())` / `.stdout(Stdio::inherit())` / `.stderr(Stdio::inherit())`
connect the child's stdio to the parent's.  The child can read from the terminal
and write to it directly; `lua` works interactively.

`.status()` runs the command and returns its exit status once it completes,
without capturing stdout/stderr.

``` {.rust #run-spawn}
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

### Propagating the exit code

!!! warning "Exit code propagation"

    Without exit code propagation, `shot run` is unusable in CI: a failing
    test would appear as a successful pipeline step.

If the child fails, we exit with the same code.  This lets you compose
`shot run` in shell scripts:

```bash
shot run lua test.lua || echo "tests failed"
```

``` {.rust #run-exit-code}
if !status.success() {
    let code = status.code().unwrap_or(1);
    std::process::exit(code);
}

Ok(())
```

`std::process::exit(code)` terminates the process immediately with the given
exit code.  It doesn't run destructors or flush buffers, but since we're about
to exit anyway, that's fine.

Why not `return Err(...)`?  There's no meaningful error to report. The child
ran successfully but indicated failure via its exit code.  Returning an error
would cause `miette` to print a message, cluttering the output.

## Summary

- `shot run` computes a modified environment via `run_activation` and spawns
  a child process with it.
- `spawn_blocking` offloads synchronous, potentially-blocking code to a
  dedicated thread pool.
- `.envs()` overlays activation variables on the inherited environment.
- `.stdin/stdout/stderr(Stdio::inherit())` gives the child full terminal access.
- Exit codes are propagated so `shot run` composes in shell scripts.

In the next chapter, the most complex one, we implement `shot build`: turning
source code into a distributable conda package.
