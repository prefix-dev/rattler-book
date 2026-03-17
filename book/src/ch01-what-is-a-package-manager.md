# Chapter 1: What Is a Package Manager?

Before writing a single line of Rust, let's make sure we agree on what a package
manager does.

## The three problems

Every package manager solves three distinct problems.  Understanding them
separately matters because each one has a different right answer.

### 1. Discovery: what exists?

Someone has written a library called `moonshine`.  You want to use it.  How does
your tool know that `moonshine` exists, what version it is, and where to download
it?

The answer is a **channel** (conda terminology) or **registry** (npm/cargo
terminology) or **index** (pip terminology): a server that publishes a catalog of
available packages.  The catalog is called **repodata** in conda.

```text
Channel: https://conda.anaconda.org/conda-forge/
  └── linux-64/
  │     repodata.json     ← catalog for 64-bit Linux packages
  └── noarch/
        repodata.json     ← catalog for architecture-independent packages
```

`repodata.json` is a large JSON file (often hundreds of megabytes) that lists
every package, every version of every package, and the dependencies of each
version.

### 2. Solving: which versions are compatible?

You ask for `lua >=5.4` and `luarocks *`.  But `luarocks` depends on
`lua >=5.1,<5.5` and your favorite library depends on `lua =5.4.*`.  Which exact
versions should be installed?

This is the **dependency solving** problem.  When a package manager enforces
that only one version of each package can be installed at a time, the problem is
NP-complete.  Russ Cox [proves this][vsat] by reducing [3-SAT] to package
version selection: each boolean variable becomes a package with two versions,
each clause becomes a package whose versions depend on the corresponding
literals, and a root package depends on all clause packages.  If the root is
installable, the formula is satisfiable.

Not every package manager hits this complexity.  If you allow multiple versions
of the same package to coexist (as Nix and Go modules do), you can install
everything the dependency graph asks for and the problem becomes much more tractable. In our case, the
hardness comes from the "exactly one version" constraint.  conda enforces that
constraint, so we need a more complex solver.

In practice, real package ecosystems have enough structure that modern SAT
heuristics solve them quickly, in most cases.  We'll use rattler's solver implementation, which is backed by
[resolvo], a pure-Rust SAT solver written by the prefix team.

[vsat]: https://research.swtch.com/version-sat
[3-SAT]: https://en.wikipedia.org/wiki/Boolean_satisfiability_problem#3-satisfiability
[resolvo]: https://github.com/mamba-org/resolvo
[cep-35]: https://conda.org/learn/ceps/cep-0035/

### 3. Installation: getting bits onto disk

Once you know *which* packages to install, you have to download and unpack them.
These are some of the things you need to think about:

- **Caching**: if you've downloaded `lua-5.4.7` before, don't download it again.
- **Deduplication**: if ten projects all use `lua-5.4.7`, rattler's approach is: store it once on disk
  and link (reflink, hardlink, etc.) it into each project's environment.
- **Transactive installation**: if the install fails halfway through, don't leave the environment
  in a broken half-installed state.
- **Platform differences**: Windows doesn't have POSIX hard links everywhere;
  macOS has a case-insensitive filesystem by default.

## What makes conda special

The conda ecosystem has properties that make it interesting to build on:

### Platform-first

Every package is compiled for a specific platform (`linux-64`, `osx-arm64`,
`win-64`, ...) and that platform is baked into the filename and metadata. A special `noarch` platform covers packages
that don't contain compiled code, like Lua packages, without c-extensions. The cost is that every platform needs its own build infrastructure and its own repodata, and there is no single artifact you can hand to all users.

### Hermetic environments

A conda environment is a self-contained directory tree.  It contains not just the
Lua library you asked for but also the exact Lua interpreter, and all of their
shared-library dependencies, pinned to specific versions.  You can have ten
projects on the same machine, each with a different Lua version, and they don't
interfere.

System package managers (`apt`, `brew`) install into global directories.  Conda
doesn't.

!!! note "The packaging trade-off"

    To keep an environment truly self-contained, you need to package everything
    it depends on, down to low-level libraries like zlib and OpenSSL that system
    package managers normally provide. That means someone has to build and
    maintain all of those packages. conda-forge makes this practical by providing
    a large shared collection of reusable recipes, so you don't have to
    repackage the world yourself.

### Virtual packages

One problem with hermetic environments: you can't install the Linux kernel into
an environment.  The environment has to run on *some* kernel, GPU driver, or glibc
version.  conda handles this with **virtual packages**, synthetic packages that
represent host-system capabilities.  You can write `__glibc >=2.17` as a
dependency and the solver will reject packages with that constraint if your glibc
is older.

## The conda file format

A conda package is an archive.  Historically it was a `.tar.bz2` file.  In the
modern [`.conda` format][cep-35] (version 2) it is an uncompressed ZIP that contains:

```text
moonshine-0.3.0-lua_0.conda
├── metadata.json          ← {"conda_pkg_format_version": 2}
├── pkg-moonshine-….tar.zst  ← payload files
└── info-moonshine-….tar.zst ← info/index.json, info/paths.json, …
```

The payload and metadata live in separate inner archives, so tools can read the
metadata without unpacking the (potentially large) payload.

Using ZIP as the outer container is a common choice. ZIP stores a central directory at the end of the file, which means a reader can seek directly to any inner entry without scanning from the beginning. This is the same reason Python wheels (`.whl`) are ZIP files: a tool can extract just the metadata entry without downloading or reading the full archive. 

We'll see both of these inner archives in detail when we build the `luapkg build` command in Chapter 9.

## Introducing: luapkg

`luapkg` is intentionally minimal.  It does not:

- **Generate lock files.** A production package manager needs them for reproducible installs, but they add file-format design, merge-conflict handling, and a separate resolution step. We discuss the concept in Chapter 5 but skip the implementation.
- **Upload to a public channel.** Publishing requires authentication, signing, and a trust model. We build packages locally and index them as a local channel instead.
- **Handle C extensions.** Supporting compiled extensions means invoking a C compiler, linking against the right libraries, and producing platform-specific artifacts. Our build command targets pure Lua only.

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
