# Chapter 1: What Is a Package Manager?

Before writing a single line of Rust, let's make sure we agree on what a package
manager actually does.  The answer is less obvious than it first appears.

## The three problems

Every package manager solves three distinct problems.  Understanding them
separately is crucial because each one has a different right answer.

### 1. Discovery: what exists?

Someone has written a library called `moonshine`.  You want to use it.  How does
your tool know that `moonshine` exists, what version it is, and where to download
it?

The answer is a **channel** (conda terminology) or **registry** (npm/cargo
terminology) or **index** (pip terminology): a server that publishes a catalog of
available packages.  The catalog is called **repodata** in conda.

```
Channel: https://conda.anaconda.org/conda-forge/
  └── linux-64/
  │     repodata.json     ← catalog for 64-bit Linux packages
  └── noarch/
        repodata.json     ← catalog for architecture-independent packages
```

`repodata.json` is a large JSON file — often hundreds of megabytes — that lists
every package, every version of every package, and the dependencies of each
version.

### 2. Solving: which versions are compatible?

You ask for `lua >=5.4` and `luarocks *`.  But `luarocks` depends on
`lua >=5.1,<5.5` and your favorite library depends on `lua =5.4.*`.  Which exact
versions should be installed?

This is the **dependency solving** problem, also known as **version SAT**.  It's
NP-hard in the worst case, but modern solvers handle practical package ecosystems
quickly using smart heuristics.

We'll use rattler's solver, which is backed by [resolvo] — a pure-Rust SAT
solver written by the pixi team.

[resolvo]: https://github.com/mamba-org/resolvo

### 3. Installation: getting bits onto disk

Once you know *which* packages to install, you have to download and unpack them.
This sounds simple but there are real complexities:

- **Caching**: if you've downloaded `lua-5.4.7` before, don't download it again.
- **Deduplication**: if ten projects all use `lua-5.4.7`, store it once on disk
  and hard-link it into each project's environment.
- **Atomicity**: if the install fails halfway through, don't leave the environment
  in a broken half-installed state.
- **Platform differences**: Windows doesn't have POSIX hard links everywhere;
  macOS has a case-insensitive filesystem by default.

## What makes conda special

The conda ecosystem has a few properties that make it particularly interesting to
build on:

### Platform-first

Every package is compiled for a specific platform (`linux-64`, `osx-arm64`,
`win-64`, ...) and that platform is baked into the filename and metadata.  There
is no "universal wheel" ambiguity.  There is, however, a special `noarch`
platform for packages that genuinely don't contain compiled code — like our Lua
packages.

### Hermetic environments

A conda environment is a self-contained directory tree.  It contains not just the
Lua library you asked for but also the exact Lua interpreter, and all of their
shared-library dependencies, pinned to specific versions.  You can have ten
projects on the same machine, each with a different Lua version, and they don't
interfere.

This is fundamentally different from system package managers (`apt`, `brew`) which
install packages into global directories.

### Virtual packages

One problem with hermetic environments: you can't install the Linux kernel into
an environment.  The environment has to run on *some* kernel, GPU driver, or glibc
version.  conda handles this with **virtual packages** — synthetic packages that
represent host-system capabilities.  You can write `__glibc >=2.17` as a
dependency and the solver will reject packages with that constraint if your glibc
is older.

## The conda file format

A conda package is an archive.  Historically it was a `.tar.bz2` file.  In the
modern `.conda` format (version 2) it is an uncompressed ZIP that contains:

```
moonshine-0.3.0-lua_0.conda
├── metadata.json          ← {"conda_pkg_format_version": 2}
├── pkg-moonshine-….tar.zst  ← payload files
└── info-moonshine-….tar.zst ← info/index.json, info/paths.json, …
```

Splitting the payload and metadata into separate inner archives means tools can
read just the metadata without unpacking (potentially large) payload files.

We'll see both of these inner archives in detail when we build the `luapkg build`
command in Chapter 9.

## Our tool: luapkg

`luapkg` is intentionally minimal.  It does not:

- Lock files (though we'll discuss them)
- Upload to a public channel
- Handle C extensions (the build command is Lua-only)

What it *does* do is wire together all four major rattler subsystems:

| Subsystem | Crate | Role |
|---|---|---|
| Repodata gateway | `rattler_repodata_gateway` | Fetch & cache channel metadata |
| Virtual packages | `rattler_virtual_packages` | Probe host system capabilities |
| Solver | `rattler_solve` | Pick consistent package versions |
| Installer | `rattler` | Download, extract, hard-link |
| Shell activation | `rattler_shell` | Generate activation scripts |

Each chapter covers one of these in depth.

## Summary

- A package manager solves three problems: discovery, version solving, and
  installation.
- conda is a platform-first, hermetic environment system with a well-defined file
  format.
- rattler implements the conda specification in pure Rust, providing library
  crates for each subsystem.
- We'll use all of them to build `luapkg`.

In the next chapter we'll set up the Rust project and define the CLI structure.
