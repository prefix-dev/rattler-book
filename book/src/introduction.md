# Introduction

<span class="newthought">This book</span> teaches you how to build a package manager.

Here is what using the finished tool will look like:

```console
$ shot init my-app --channel conda-forge --channel ../channel
$ shot add lumen
$ shot install
  1523 repodata records loaded
  Solved 12 packages in 0.3s
✔ Wrote moonshot.lock (12 packages)
✔ Environment updated in 2.1s
  Activate with:  eval $(shot shell-hook)

$ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
```

`lumen` is a Lua library we wrote ourselves. It depends on ImageMagick, a C
library with dozens of native dependencies. Both were solved, installed, and
activated by our package manager. Where did `lumen` come from? We built it
with moonshot's other headline command:

```console
$ shot build --output-dir ../channel
Building lumen 0.1.0 (build lua_0)
  → Installing 1 build dependencies…
  → Running build script `build.lua`
  → Packing 2 files…
  → Indexing channel at ../channel
✔ Built lumen-0.1.0-lua_0.conda
  package → ../channel/noarch/lumen-0.1.0-lua_0.conda
  channel → ../channel
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

## Who am I?

This book was mostly produced by me, [Tim de Jager](https://github.com/tdejager/). Make sure to read [this](#note-on-generative-ai-usage) section though. 

I'm a main contributor on [pixi] and [rattler] and I work for [prefix.dev](https://prefix.dev/). Previously I worked
in both robotics and gaming before switching to package managing. 

We are very bullishly convinced that conda can serve as not only a good base for building libraries and applications,
but also for package managers in general. A conda environment is basically an isolated linux-prefix! So we really
wanted to start sharing this with the world.

## How this book is organized

**Part I** will build `moonshot` from scratch. Each chapter will implement one
command from start to finish. Mostly following the following order: first the design, then configuration changes,
then concepts, then implementation.

**Part II** allows you to dive deeper into the rattler library itself: the package
format, the SAT solver, the networking stack. These chapters stand alone, you
can read them in any order.

## Note on Generative AI usage
Generative AI, specifically: Claude Opus, was extensively used in the production of this book and to verify that the exercises can be completed. Claude has also been used for writing initial drafts of both code and prose. I did try to set my own rules for Claude to follow and I have done, many, many iterations on the generated text alone and with Claude. All text was heavily edited, appended, and changed by myself to make it as concise as possible.

I want to be honest w.r.t to this fact so that you as a reader can decide for yourself how to proceed. This section has been written entirely by hand. Any other way would feel disingenuous to me.

## What we will build

We are going to build `moonshot`, a minimal Lua package manager. By the final chapter it will be able to:

```console
$ shot init my-app          # scaffold a new project
$ shot add lua ">=5.4"      # add dependencies
$ shot install              # fetch, solve, and install
$ shot shell-hook           # activate the environment
$ shot run lua script.lua   # run inside the environment
$ shot build                # build a distributable package
```

The full source code lives in the `src/` directory alongside this book in the
[rattler-book repository][repo].


## Running the examples

The easiest way to get started is with [pixi](https://pixi.prefix.dev/latest/installation/), which manages the Rust toolchain
and all dependencies for you:

```bash
pixi install
pixi run build
```

I know it can be a hassle to install a new tool, I really do! But using pixi can famliarize you with what rattler can empower to build.
Additionally, it will hopefully convince you that its a very useful tool as well.

The `run` command utilizes pixi's tasks system to run tasks. In a way the task system is similar to [mise](https://mise.jdx.dev/tasks/) or [just](https://github.com/casey/just). 

In out project we use a [`[dev]`](https://pixi.prefix.dev/latest/build/dev/) table in `pixi.toml` to pull in the Rust compiler and
all build dependencies automatically via the `pixi-build-rust` backend.
These can then be used in conjunction with `run`.
No manual Rust installation required.

You can also build a distributable conda package:

```bash
pixi build
```

You can also install `moonshot` globally

```bash
pixi global install --path .
```

Again, pixi automatically figures out what to install and how to build it.

To test things its easy to run:
```bash
pixi r shot <cmd>
# e.g
pixi r shot init
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
git clone https://github.com/prefix-dev/rattler-book
cd rattler-book
cargo build
```

[rustup]: https://rustup.rs

## Literate programming

This book uses [Entangled], a literate programming tool. The code blocks in
each chapter are the actual source code. Entangled extracts them from the
Markdown into real files that compile and run.

Code blocks that produce a file carry a `file=` attribute. Here is a small (fake)
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

Do you also see the small <button class="source-link-btn" data-path="examples/intro/hello.rs" title="View full file" aria-label="View examples/intro/hello.rs in source viewer">&lt;/&gt;</button>
in the heading? Click that to go the file directly, using a built-in file browser.

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


## Why build on conda?

Here's why we chose conda as the foundation:

- **Existing packages.** [conda-forge] has thousands of packages across Python,
  R, C++, Fortran, etc.  Your language's packages can depend on native libraries
  that are already packaged on conda-forge.
- **Binary distribution.** Packages ship as prebuilt binaries per platform. There is no
  compilation on the user's machine.
- **Isolated environments.** Environments can be isolated from each other, 
  think of python `.venv` for example but with more native dependency types.
- **Mature tooling.** [rattler] provides a solver, installer, networking stack, and
  shell activation in reusable Rust crates.

That said, conda isn't always the right choice:

- **One version per package** means dependency resolution is NP-complete and
  requires a SAT solver.  If your ecosystem can tolerate duplicate versions (like
  Go or Nix), you avoid that complexity.

[rattler]: https://github.com/conda/rattler
[repo]: https://github.com/prefix-dev/rattler-book
[Lua]: https://www.lua.org
[conda-forge]: https://conda-forge.org

Continue on to the next chapter to get started. Or read below for more details regarding exercises in the book.

## Exercises

Most chapters include programming exercises. These are marked by difficulty, the following are examples:

!!! exercise-easy "Hello, rattler"

    Write a short Rust program that prints the version of the `rattler_conda_types`
    crate. This is a warm-up to make sure your toolchain is set up correctly.

    Expected behavior
    :   The program compiles and prints a version string such as `0.29.0`.

!!! exercise-intermediate "Parse a MatchSpec"

    /// margin-note
    Look at the `MatchSpec` type in `rattler_conda_types`. The `from_str`
    method does most of the work.
    ///

    Given the string `python >=3.8,<4.0`, write a function that extracts the
    package name and version constraint using `MatchSpec::from_str`.

    Expected behavior
    :   Your function returns the package name `python` and constraint `>=3.8,<4.0`
        as separate values.

!!! exercise-hard "Version ordering from scratch"

    Without using rattler's built-in comparison, implement the conda version
    ordering algorithm for simple numeric versions like `1.2.3` vs `1.10.0`.

    Expected behavior
    :   Your comparison function agrees with `Version::from_str` ordering for all
        pairs in a test suite of at least ten version strings.

### Exercises with an AI tutor

The repository includes `TUTOR.md`, a system prompt that turns a coding agent
into a guided tutor for the exercises. The tutor will **never** write code for you.
Instead, it asks questions, points you to relevant files, and reveals hints
one step at a time as you work through each exercise.
/// margin-note
You need to start talking to the agent, before it can start using the system-prompt
///

Before you start an exercise, the tutor will ask how comfortable you are with
Rust. If you are learning Rust alongside this book, the tutor will explain
language concepts and link to documentation as they come up. If you already know
Rust, it will focus on the rattler APIs and the moonshot codebase.

To start a tutoring session, load `TUTOR.md` as a system prompt in your agent
of choice:

**Claude Code**

```bash
claude --append-system-prompt-file TUTOR.md
```

**Cursor**

Create `.cursor/rules/tutor.mdc` with `alwaysApply: true` in the frontmatter
and paste the contents of `TUTOR.md` as the body.

**GitHub Copilot**

Copy the contents of `TUTOR.md` into `.github/copilot-instructions.md` at the
repository root.

**OpenAI Codex CLI**

Copy the contents of `TUTOR.md` into `.codex/instructions.md` at the
repository root.

**Other agents**

Copy the contents of `TUTOR.md` into your agent's system prompt or custom
instructions field.

Once the session starts, tell the tutor which exercise you want to work on
(e.g., "exercise 3.1") and it will guide you from there.
