# Chapter 8: Running Commands in the Environment

`luapkg shell` requires the user to evaluate shell-specific output. That works for interactive use but creates two problems: it is awkward to use in scripts and CI pipelines, and it ties you to a specific shell dialect. `luapkg run` solves both. It computes the activated environment internally and spawns the command as a child process, so it works the same way regardless of whether the user runs Bash, Fish, PowerShell, or no shell at all.

```bash
luapkg run lua -e 'print("hello from conda")'
luapkg run luarocks install inspect
```

## The strategy: environment diffing

We can't modify the parent shell's environment, but we *can* control the
environment of a child process.  The trick is:

1. Compute what the environment *would* look like after activation.
2. Spawn the user's command with that modified environment.

The child inherits the modified environment; the parent is untouched.

This uses the same activation logic from Chapter 7, but instead of printing a script it captures the resulting environment as a map of variable names to values.

rattler provides this via `Activator::run_activation`:

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

## Why `spawn_blocking`?

`run_activation` is synchronous; it shells out and waits for the child process
to complete.  In an async context (Tokio), blocking the thread stalls the
entire thread pool worker, starving other futures.

`tokio::task::spawn_blocking` moves the closure to a dedicated thread from
Tokio's blocking thread pool (the `max_blocking_threads` pool we configured in
`main`).  The async code `await`s the result without blocking.

```rust
tokio::task::spawn_blocking(move || {
    // This runs on a blocking thread, safe to block here
    activator.run_activation(current_vars, None)
})
.await              // async wait for the blocking thread to finish
.into_diagnostic()? // JoinError: the blocking thread panicked
.into_diagnostic()? // ActivationError: activation itself failed
```

The double `.into_diagnostic()?` handles two distinct failure modes:
- The `JoinError` from `spawn_blocking` (the closure panicked).
- The `ActivationError` from `run_activation` itself.

## Spawning the child process

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

## Propagating the exit code

Without exit code propagation, `luapkg run` is unusable in CI: a failing test would appear as a successful pipeline step.

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
