<p align="center">
  <img src="book/src/assets/paxton-moon.png" alt="The Rattler Book" width="400" />
</p>

<h1 align="center">The Rattler Book</h1>

<p align="center">
  <strong>Building a package manager with <a href="https://github.com/conda/rattler">Rattler</a></strong>
  <br />
  <a href="https://prefix-dev.github.io/rattler-book/">Read the book online</a> | <a href="https://github.com/prefix-dev/rattler-book">GitHub Repository</a>
</p>

---

This repository contains **The Rattler Book** — a hands-on guide to building conda-compatible package management tools in Rust using the [Rattler](https://github.com/conda/rattler) framework. The book walks you through creating **moonshot**, a minimal Lua package manager, from scratch.

## What you'll learn

The book covers the full lifecycle of a package manager:

1. **Parsing** dependency specs from a project manifest
2. **Fetching** repodata from conda channels via `rattler_repodata_gateway`
3. **Solving** dependencies using the resolvo SAT solver
4. **Installing** packages with hard-linking from cache
5. **Activating** environments for shell sessions or command execution
6. **Building** new conda packages with a Lua-based build system

## Book chapters

| # | Chapter | Topic |
|---|---------|-------|
| 1 | What Is a Package Manager? | Core concepts and terminology |
| 2 | Setting Up the Project | Scaffolding with Rattler crates |
| 3 | The `init` Command | Creating a project manifest |
| 4 | The `search` Command | Querying conda channels |
| 5 | The `add` Command | Adding packages to the manifest |
| 6 | The `lock` Command | Dependency resolution and lock files |
| 7 | The `install` Command | Downloading and linking packages |
| 8 | The `shell-hook` Command | Shell activation scripts |
| 9 | The `run` Command | Running commands in an environment |
| 10 | The `build` Command | Building `.conda` packages |

Plus deep-dive chapters on the conda package format, run exports, virtual packages, networking, the resolvo SAT solver, the build script API, the full Rattler crate ecosystem, and adapting moonshot to your own language.

## The moonshot CLI

The worked example — **moonshot** — is a fully functional Lua package manager:

```bash
shot init                    # Create a new project
shot search lua              # Search for packages
shot add lua ">=5.4"         # Add a package to the manifest
shot lock                    # Resolve dependencies and write the lock file
shot install                 # Install packages into the environment
shot shell-hook              # Print a shell activation script
shot run lua my_script.lua   # Run a command in the environment
shot build                   # Build a .conda package
```

## Repository structure

```
book/                    # The Rattler Book (mkdocs documentation)
src/                     # moonshot source code (Rust)
examples/                # Example Lua packages (moonjson, mooncolor, moontemplate, hello-moon)
```

## Getting started

Read the book online at **[prefix-dev.github.io/rattler-book](https://prefix-dev.github.io/rattler-book/)**, or build it locally:

```bash
pixi run book-serve
```

To build the moonshot CLI:

```bash
pixi run build
```

Or without pixi (requires Rust 1.82+):

```bash
cargo build
```

## License

This project is part of the [prefix-dev](https://github.com/prefix-dev) ecosystem.
