# Introduction

This book teaches you how to build a package manager.

It installs, caches, solves version conflicts, and activates environments on
Windows, macOS, and Linux.  We build it on top of **rattler**, a library that
implements the [conda] package specification in pure Rust.  By the end you will
have a working CLI tool and a thorough understanding of every decision that went
into it.

[conda]: https://docs.conda.io/projects/conda/en/stable/

## Who this is for

This book is aimed at programmers who:

- Are curious about how package managers work under the hood.
- Want to understand the rattler library and the conda package ecosystem.
- Are considering building a package manager for their own programming language.
  Conda's language-agnostic format makes it a surprisingly good foundation.

You don't need to know anything about conda, packaging, or the Lua programming
language.  We use Lua as the target language because it's small and
self-contained, but the techniques generalize to any ecosystem.

## What we build

`luapkg`, a minimal Lua package manager.  It can:

```console
$ luapkg init my-app          # scaffold a new project
$ luapkg add lua ">=5.4"      # add dependencies
$ luapkg install              # fetch, solve, and install
$ luapkg shell                # activate the environment
$ luapkg run lua script.lua   # run inside the environment
$ luapkg build                # build a distributable package
```

The final source is in the `src/` directory alongside this book.

## How this book is organized

**Part I** builds `luapkg` from scratch, command by command.  Each chapter
walks through one part of the implementation.

**Part II** dives deeper into the rattler library itself: the package format,
the SAT solver, the networking stack.  These chapters stand alone; you can read
them in any order.

## Running the examples

The easiest way to get started is with [pixi], which manages the Rust toolchain
and all dependencies for you:

```console
pixi install
pixi run build
```

pixi uses the `[dev]` table in `pixi.toml` to pull in the Rust compiler and
all build/host dependencies automatically via the `pixi-build-rust` backend.
No manual Rust installation required.

You can also build a distributable conda package:

```console
pixi build
```

[pixi]: https://pixi.sh

### Without pixi

If you prefer to manage Rust yourself, you need Rust 1.82 or later.  Install it
with [rustup]:

```console
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then clone the repository and build:

```console
git clone https://github.com/mamba-org/rattler-book
cd rattler-book
cargo build
```

[rustup]: https://rustup.rs

## Why build on conda?

Reasons to choose conda as a foundation for a new package manager:

- **Existing packages.** [conda-forge] has thousands of packages across Python,
  R, C++, Fortran, etc.  Your language's packages can depend on native libraries
  that are already packaged.
- **Binary distribution.** Packages ship as prebuilt binaries per platform.  No
  compilation on the user's machine.
- **Consistent environments.** One version per package per environment.  All
  binaries link against the same set of libraries.
- **Mature tooling.** rattler provides a solver, installer, networking stack, and
  shell activation in reusable Rust crates.

Reasons you might not:

- **One version per package** means dependency resolution is NP-complete and
  requires a SAT solver.  If your ecosystem can tolerate duplicate versions (like
  Go or Nix), you avoid that complexity.
- **Large binary packages.** Conda packages include compiled artifacts.  If your
  language is source-only or has a fast compiler, source distribution may be
  simpler.
[conda-forge]: https://conda-forge.org

Let's get started.
