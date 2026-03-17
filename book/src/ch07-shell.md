# Chapter 7: Shell Activation

After installing packages into `.luapkg/env/`, the user needs to be able to
*use* them.  That means getting `lua`, `luarocks`, and any other installed
binaries onto their `PATH`, and setting any environment variables that packages
declare.

This is the **activation** problem, and it's trickier than it looks.

## Why activation is non-trivial

Package managers handle activation in different ways. Some use shims (small wrapper executables that redirect to the right version), others use wrapper scripts that set `PATH` before invoking the tool. conda uses eval-based activation: the tool prints a shell script and the user evaluates it in their current shell. This gives packages full control over the environment (not just `PATH` but also `LD_LIBRARY_PATH`, `LUA_PATH`, and any other variable), at the cost of being shell-dependent.

A child process cannot modify the environment of its parent.  That's a Unix
rule with no exceptions.  So when you run `luapkg shell`, it can't just set
`PATH` for you; it has to print a script that you evaluate in your shell.

`luapkg shell` and `luapkg run` (Chapter 8) represent a design fork. `shell` generates a script the user evaluates, which means it must know the user's shell dialect. `run` spawns a child process with the right environment variables, which is shell-agnostic but only lasts for one command. Most package managers end up needing both.

```bash
eval $(luapkg shell)         # bash / zsh
luapkg shell | source        # fish
```

The `eval` trick runs the output of `luapkg shell` as shell code in the current
process, which *can* modify `PATH`.

But it gets more complicated:

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

## How `luapkg shell` works

```rust
pub fn execute(args: Args) -> miette::Result<()> {
    let cwd = env::current_dir().into_diagnostic()?;
    let prefix = /* resolve prefix path */;

    let platform = Platform::current();

    // 1. Detect which shell to emit
    let shell: ShellEnum = if let Some(ref name) = args.shell {
        ShellEnum::from_str(name)
            .map_err(|_| miette::miette!("Unknown shell `{name}`"))?
    } else {
        ShellEnum::from_env().unwrap_or_else(|| Bash.into())
    };

    // 2. Build the Activator
    let activator = Activator::from_path(&prefix, shell, platform)
        .into_diagnostic()?;

    // 3. Read current activation state
    let vars = ActivationVariables::from_env().into_diagnostic()?;

    // 4. Generate the activation script
    let result = activator.activation(vars).into_diagnostic()?;
    let script = result.script.contents().into_diagnostic()?;

    print!("{script}");
    Ok(())
}
```

Note that `execute` is *not* `async`.  Generating an activation script is
synchronous: no network, no disk I/O beyond reading a few small files in the
prefix.

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
`Activator` calls these methods generically; it doesn't need to know which shell
it's generating for.

### Detecting the shell from `$SHELL`

```rust
ShellEnum::from_env().unwrap_or_else(|| Bash.into())
```

`from_env()` reads `$SHELL` (on Unix) or the default shell (on Windows) and
returns `Option<ShellEnum>`.  If the shell isn't recognized, we fall back to
Bash, which has the widest compatibility.

`Bash.into()` uses the `Into` trait to convert `Bash` (the unit struct) into
`ShellEnum::Bash(Bash)`.  The `.into()` method is available whenever `From` is
implemented; `From<Bash> for ShellEnum` is implemented by rattler.

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

For Bash, `luapkg shell` might print something like:

```bash
export PATH="/home/user/my-app/.luapkg/env/bin:$PATH"
export CONDA_PREFIX="/home/user/my-app/.luapkg/env"
export CONDA_SHLVL="1"
export CONDA_DEFAULT_ENV="/home/user/my-app/.luapkg/env"
# source activation scripts
. "/home/user/my-app/.luapkg/env/etc/conda/activate.d/lua_path.sh"
```

The user evaluates this, and from that point `lua`, `luarocks`, etc. are on their
PATH.

You might notice that a Lua package manager is setting `CONDA_PREFIX` and `CONDA_SHLVL`. This is because rattler implements conda's activation protocol, and these variables are part of that protocol. The benefit is compatibility: any tool that understands conda environments (editors, CI systems, other package managers) will recognize our environment. The cost is the naming confusion, since "CONDA" has nothing to do with Lua. A production tool could alias these variables, but you would lose the ecosystem compatibility.

## Summary

- Shell activation generates a script that the user evaluates to modify their
  shell's environment.
- `rattler_shell` handles multi-shell compatibility (Bash, Fish, PowerShell, ...).
- `Activator::from_path` reads activation metadata from the prefix.
- `ActivationVariables` captures current state for correct nested-activation
  handling.

In the next chapter we implement `luapkg run`, a way to run a command inside the
activated environment without permanently modifying the shell.
