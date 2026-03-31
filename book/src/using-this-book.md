# Using This Book

## Getting the source

Clone the repository:

```bash
git clone https://github.com/prefix-dev/rattler-book
cd rattler-book
```

## Using the book's source

The easiest way to get started is with [pixi](https://pixi.prefix.dev/latest/installation/), which manages the Rust toolchain
and all dependencies for you.  I know it can be a hassle to install a new tool. I really do! But using pixi can familiarize you with what rattler can empower to build.
Additionally, it will hopefully convince you that it's a very useful tool as well.

To test things its easy to run:
```bash
pixi r shot <cmd>
# e.g
pixi r shot init
```

The `run` command uses pixi's tasks system to run tasks. In a way the task system is similar to [mise](https://mise.jdx.dev/tasks/) or [just](https://github.com/casey/just). 

To just build without running:

```bash
# Installs the pixi env and runs the build
pixi run build
```

In our project we use a [`[dev]`](https://pixi.prefix.dev/latest/build/dev/) table in `pixi.toml` to pull in the Rust compiler and
all build dependencies automatically via the `pixi-build-rust` backend.
These can then be used in conjunction with `run`.
No manual Rust installation required.

You can also build a distributable conda package:

/// margin-note
`pixi run build` runs the build task. While `pixi build` is built-in.
///
```bash
pixi build
```

You can also install `moonshot` globally:

```bash
pixi global install --path .
# it will be available as
shot <cmd>
```

Again, pixi automatically figures out what to install and how to build it.

[pixi]: https://pixi.sh

### Without pixi

If you prefer to manage Rust yourself, you need Rust 1.82 or later.  Install it
with [rustup] and build with cargo:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
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

/// margin-note
In literate programming the `<<code>>` were popularized by the [noweb](https://www.cs.tufts.edu/~nr/noweb/) tool.
///
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

Every code block that carries a `file=` or `#name` attribute has a small
<button class="source-link-btn" style="pointer-events:none">&lt;/&gt;</button>
button in its title bar. Clicking it opens the **source viewer**, a slide-up
panel where you can browse the full tangled files. The viewer scrolls directly
to the corresponding section and highlights it briefly. You can also open the
source viewer any time with the
<code>&lt;/&gt;</code> icon in the header bar.

Inside the tangled files you will see marker comments like
`// ~/~ begin <<intro-imports>>[init]` that record where each block came from.
They act as section separators; click the fold arrow in the gutter to collapse
any section you are not interested in.

You can tangle all files with:

```bash
pixi run tangle
```

And stitch source-file edits back into the Markdown with:

```bash
pixi run stitch
```

[Entangled]: https://entangled.github.io/

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
