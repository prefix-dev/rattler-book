# luapkg

A minimal Lua package manager built on the [Rattler](https://github.com/conda/rattler) framework. This is a worked example that accompanies **The Rattler Book**, demonstrating how to build conda-compatible package management tools in Rust.

## What it does

luapkg installs Lua packages from conda channels (primarily conda-forge), creates isolated environments, and includes a build system for creating new Lua packages.

## Commands

| Command   | Description                                         |
|-----------|-----------------------------------------------------|
| `init`    | Create a new `luapkg.toml` project manifest         |
| `add`     | Add a package to the manifest and install it         |
| `install` | Resolve and install all dependencies                 |
| `shell`   | Print shell activation script for the environment    |
| `run`     | Execute a command inside the activated environment    |
| `build`   | Build a `.conda` package from a `recipe.toml`        |

## Quick start

```bash
# Build from source
cargo build --release

# Initialize a new project
luapkg init

# Add a package
luapkg add lua ">=5.4"

# Install dependencies
luapkg install

# Run a Lua script in the environment
luapkg run lua my_script.lua
```

## Project structure

```
src/
├── main.rs              # CLI entry point
├── manifest.rs          # luapkg.toml parsing
├── recipe.rs            # recipe.toml (build recipe) parsing
├── progress.rs          # Spinner utilities
├── build_prelude.lua    # Lua helpers embedded into build scripts
└── commands/
    ├── init.rs          # Project initialization
    ├── add.rs           # Add packages
    ├── install.rs       # Dependency resolution & installation
    ├── shell.rs         # Shell activation
    ├── run.rs           # Run commands in environment
    └── build.rs         # Package building

book/                    # mdBook documentation (The Rattler Book)
examples/                # Example Lua packages (moonjson, mooncolor, moontemplate, hello-moon)
```

## How it works

1. **Parse** dependency specs from `luapkg.toml`
2. **Fetch** repodata from conda channels via `rattler_repodata_gateway`
3. **Solve** dependencies using the resolvo SAT solver
4. **Install** packages into `.luapkg/env/` with hard-linking from cache
5. **Activate** the environment for shell sessions or command execution

## Examples

Four example Lua packages demonstrate the build system:

- **moonjson** — JSON library
- **mooncolor** — Color utilities
- **moontemplate** — Template engine
- **hello-moon** — Demo app using all three

Each contains a `recipe.toml` and a `build.lua` script.

## Documentation

See the `book/` directory for full documentation covering package manager concepts, the solver, installation, shell activation, building packages, and more.
