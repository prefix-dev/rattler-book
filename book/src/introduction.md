# Introduction

<span class="newthought">This book</span> teaches you how to build a package manager.

Here is what using the finished tool will look like:

```console
$ shot init my-app --channel conda-forge --channel ../channel
$ shot add lumen imagemagick
$ shot install
✔ Installed 12 packages

$ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
```

`lumen` is a Lua library we wrote ourselves. ImageMagick is a C library with
dozens of native dependencies. Both were solved, installed, and activated by
our package manager. Where did `lumen` come from? We built it with moonshot's
other headline command:

```console
$ shot build --output-dir ../channel
✔ Built lumen-0.1.0-lua_0.conda
```

By the end of this book, you will have built the tool that did all of that, on
top of **[rattler]**, a library that implements the [conda] package
specification in pure Rust.

[conda]: https://docs.conda.io/projects/conda/en/stable/

## Who this is for

This book is aimed at programmers who:

- Are curious about how package managers work under the hood.
- Want to understand the rattler library and the conda package ecosystem.
- Are considering building a package manager for their own programming language.
  Conda's language-agnostic format makes it a surprisingly good foundation.

You don't need to know anything about conda, packaging, or the Lua programming
language.  We use [Lua] as the target language because it's small and
self-contained, but the techniques generalize to any ecosystem.

## What we will build

`moonshot`, a minimal Lua package manager. By the final chapter it will be able to:

```console
$ shot init my-app          # scaffold a new project
$ shot add lua ">=5.4"      # add dependencies
$ shot install              # fetch, solve, and install
$ shot shell                # activate the environment
$ shot run lua script.lua   # run inside the environment
$ shot build                # build a distributable package
```

The final source will live in the `src/` directory alongside this book.

## How this book is organized

**Part I** will build `moonshot` from scratch. Each chapter will implement one
command from start to finish: first the design, then configuration changes,
then concepts, then implementation.

**Part II** will dive deeper into the rattler library itself: the package
format, the SAT solver, the networking stack. These chapters stand alone; you
can read them in any order.

## Literate programming

This book uses [Entangled], a literate programming tool. The code blocks in
each chapter are the actual source code. Entangled extracts them from the
Markdown into real files that compile and run.

Code blocks that produce a file carry a `file=` attribute. Here is a small
program we will use to show how it works:

``` {.rust file=examples/intro/hello.rs}
<<intro-imports>>

fn main() {
    <<intro-prompt-greet>>
}
```

The `<<intro-prompt-greet>>` placeholder refers to a named block. When the same
name appears on multiple code blocks, Entangled appends them in order. This
lets us explain each piece separately while they end up as one continuous block
in the tangled output.

The imports bring in `std::io` for reading from stdin:

``` {.rust #intro-imports}
use std::io::{self, Write};
```

First we print a prompt and flush stdout so it appears before we block on input:

``` {.rust #intro-prompt-greet}
print!("what is your name? ");
io::stdout().flush().unwrap();
```

Then we read a line and print a greeting:

``` {.rust #intro-prompt-greet}
let mut name = String::new();
io::stdin().read_line(&mut name).unwrap();
println!("hello, {}!", name.trim());
```

Entangled stitches these together: every `<<intro-imports>>` reference is
replaced by the contents of the `#intro-imports` block. This lets the book
introduce code in the order that makes sense for reading, while the tangled
output follows the order the compiler expects.

You can tangle all files with:

```bash
pixi run tangle
```

And stitch source-file edits back into the Markdown with:

```bash
pixi run stitch
```

[Entangled]: https://entangled.github.io/

## Running the examples

The easiest way to get started is with [pixi], which manages the Rust toolchain
and all dependencies for you:

```bash
pixi install
pixi run build
```

pixi uses the `[dev]` table in `pixi.toml` to pull in the Rust compiler and
all build/host dependencies automatically via the `pixi-build-rust` backend.
No manual Rust installation required.

You can also build a distributable conda package:

```bash
pixi build
```

[pixi]: https://pixi.sh

### Without pixi

If you prefer to manage Rust yourself, you need Rust 1.82 or later.  Install it
with [rustup]:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then clone the repository and build:

```bash
git clone https://github.com/mamba-org/rattler-book
cd rattler-book
cargo build
```

[rustup]: https://rustup.rs

## Why build on conda?

Here's why we chose conda as the foundation:

- **Existing packages.** [conda-forge] has thousands of packages across Python,
  R, C++, Fortran, etc.  Your language's packages can depend on native libraries
  that are already packaged.
- **Binary distribution.** Packages ship as prebuilt binaries per platform.  No
  compilation on the user's machine.
- **Consistent environments.** One version per package per environment.  All
  binaries link against the same set of libraries.
- **Mature tooling.** rattler provides a solver, installer, networking stack, and
  shell activation in reusable Rust crates.

That said, conda isn't always the right choice:

- **One version per package** means dependency resolution is NP-complete and
  requires a SAT solver.  If your ecosystem can tolerate duplicate versions (like
  Go or Nix), you avoid that complexity.
- **Large binary packages.** Conda packages include compiled artifacts.  If your
  language is source-only or has a fast compiler, source distribution may be
  simpler.
[rattler]: https://github.com/mamba-org/rattler
[Lua]: https://www.lua.org
[conda-forge]: https://conda-forge.org

Let's get started.
