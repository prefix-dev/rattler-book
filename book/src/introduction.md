# Introduction

This book teaches you how to build a package manager.

Not a toy one.  A real one — one that can install, cache, solve version
conflicts, and activate environments on Windows, macOS, and Linux.  We build it
on top of **rattler**, a library that implements the [conda] package
specification in pure Rust.  By the end you will have a working CLI tool and a
thorough understanding of every decision that went into it.

[conda]: https://docs.conda.io/projects/conda/en/stable/

## Who this is for

This book is aimed at programmers who:

- Know *some* Rust — you've read a few chapters of [The Book] and written some
  small programs, but you don't consider yourself an expert.
- Are curious about how package managers actually work under the hood.
- Want a practical project that touches real-world Rust: async I/O, error
  handling, serialization, process management, and more.

[The Book]: https://doc.rust-lang.org/book/

You don't need to know anything about conda, packaging, or the Lua programming
language.  We use Lua as the target language because it's small and
self-contained, but the techniques generalize to any ecosystem.

## What we build

`luapkg` — a minimal Lua package manager.  It can:

```
$ luapkg init my-app          # scaffold a new project
$ luapkg add lua ">=5.4"      # add dependencies
$ luapkg install              # fetch, solve, and install
$ luapkg shell                # activate the environment
$ luapkg run lua script.lua   # run inside the environment
$ luapkg build                # build a distributable package
```

The final source is in the `src/` directory alongside this book.  Every chapter
walks through one part of the implementation.

## How this book is organized

**Part I** builds `luapkg` from scratch, command by command.  Each chapter
introduces both the Rust concept needed and the package-manager concept being
implemented.

**Part II** dives deeper into the rattler library itself — the package format,
the SAT solver, the networking stack.  These chapters stand alone; you can read
them in any order.

The **Appendix** collects Rust concepts that come up repeatedly so you have a
single place to look them up.

## Running the examples

You need Rust 1.82 or later.  Install it with [rustup]:

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then clone the repository and build:

```
git clone https://github.com/mamba-org/rattler-book
cd rattler-book
cargo build
```

[rustup]: https://rustup.rs

## A note on the conda ecosystem

conda is both a file format and a convention.  Thousands of packages for Python,
R, C++, and other ecosystems are already available on [conda-forge].  By
building on rattler we immediately get access to that entire ecosystem — `luapkg`
doesn't need its own package repository because conda-forge already has Lua and
LuaRocks packaged.

[conda-forge]: https://conda-forge.org

This is one of the great under-appreciated advantages of building on an existing
package format: you inherit years of packaging work for free.

Let's get started.
