# Chapter 9: The `run` Command

`shot shell` requires you to evaluate shell-specific output. That works for interactive use but creates two problems: it's awkward in scripts and CI pipelines, and it ties you to a specific shell dialect. `shot run` solves both. It computes the activated environment internally and spawns the command as a child process, so it works the same way regardless of whether you use Bash, Fish, PowerShell, or no shell at all.

## Design

```bash
shot run lua -e 'print("hello from lumen-app")'
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

The child inherits the modified environment; the parent is untouched. This is the same pattern [pixi] uses for `pixi run`.

This uses the same activation logic from [Chapter 8](ch08-shell-hook.md), but instead of printing a script it captures the resulting environment as a map of variable names to values. Because [rattler]'s `run_activation` executes the full activation sequence (including any `activate.d` scripts that packages ship), dynamic environment variables like `PKG_CONFIG_PATH` and `LUA_PATH` are picked up automatically.

## Implementation

### Adding `activation_env` to `Environment`

The `Environment` struct from [Chapter 8](ch08-shell-hook.md) already handles
shell activation scripts. For `shot run`, we need a different view: instead of
a script to evaluate, we need the full set of environment variables as a map.
We add an `activation_env` method that appends to `src/environment.rs`:

``` {.rust file=src/environment.rs}
<<environment-activation-env>>
```

``` {.rust #environment-activation-env}
impl Environment {
    /// Compute the full set of environment variables that activation
    /// would produce.
    pub async fn activation_env(&self) -> miette::Result<HashMap<String, String>> {
        let prefix = self.prefix.clone();
        let platform = self.platform;

        tokio::task::spawn_blocking(move || {
            let shell: ShellEnum = ShellEnum::from_env().unwrap_or_else(|| Bash.into());
            let activator =
                Activator::from_path(&prefix, shell, platform).into_diagnostic()?;
            let vars = ActivationVariables::from_env().into_diagnostic()?;
            activator.run_activation(vars, None).into_diagnostic()
        })
        .await
        .into_diagnostic()?
    }
}
```

rattler's `Activator::run_activation` works by writing a temporary shell script
that:

1. Emits the current environment (via `env` on Unix, `set` on Windows).
2. Sources the activation logic.
3. Emits the environment again.

It then runs that script and diffs the two snapshots, returning only the changed
variables as a `HashMap<String, String>`.

We use `spawn_blocking` because `run_activation` spawns a synchronous child
process internally. The [tokio] runtime manages the blocking thread pool.

### The run command

With `Environment` handling activation, the run command becomes straightforward:

``` {.rust file=src/commands/run.rs}
use std::process::Stdio;

use clap::Parser;
use miette::IntoDiagnostic;
use tokio::process::Command;

use crate::environment::Environment;
use crate::project::Project;

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
    let project = Project::discover()?;
    let env = Environment::from_project(&project, args.prefix)?;
    env.ensure_exists()?;

    let activation_env = env.activation_env().await?;

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

### Spawning the child process

We use [tokio]'s `tokio::process::Command` (the async version of `std::process::Command`)
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

### Propagating the exit code

<details class="margin-note" markdown>
<summary>Exit code propagation</summary>

Without exit code propagation, `shot run` is unusable in CI: a failing
test would appear as a successful pipeline step.
</details>

If the child fails, we exit with the same code.  This lets you compose
`shot run` in shell scripts:

```bash
shot run lua test.lua || echo "tests failed"
```

`std::process::exit(code)` terminates the process immediately with the given
exit code.  It doesn't run destructors or flush buffers, but since we're about
to exit anyway, that's fine.

Why not `return Err(...)`?  There's no meaningful error to report. The child
ran successfully but indicated failure via its exit code.  Returning an error
would cause `miette` to print a message, cluttering the output.

<!-- TODO: Exercises
- Run `shot run env | grep CONDA` to see what activation variables are set.
- Try `shot run lua -e 'print(package.path)'` to see the Lua module search path. Does it include `.env/share/lua/5.4/`?
- Run `shot run false` (a command that exits with code 1). What exit code does `shot` return?
-->

## Summary

- `shot run` computes a modified environment via `run_activation` and spawns
  a child process with it.
- [tokio]'s `spawn_blocking` offloads synchronous, potentially-blocking code to a
  dedicated thread pool.
- `.envs()` overlays activation variables on the inherited environment.
- `.stdin/stdout/stderr(Stdio::inherit())` gives the child full terminal access.
- Exit codes are propagated so `shot run` composes in shell scripts.

In the next chapter, the most complex one, we implement `shot build`: turning
source code into a distributable conda package.

[pixi]: https://pixi.sh
[rattler]: https://github.com/conda/rattler
[tokio]: https://tokio.rs
