# Introduction

<span class="newthought">This book</span> teaches you how to build a package manager.
A package manager automates finding, downloading, and installing software libraries so you don't have to track versions and dependencies by hand.

Here is what using the finished tool will look like:

```console
# Create a new project
$ shot init my-app --channel ../channel --channel conda-forge
# Add any conda dependency
$ shot add lumen
# Install your environment
$ shot install
  47255 repodata records loaded
  Solved 85 packages in 0.8s
✔ Wrote moonshot.lock (85 packages)
✔ Environment updated
  Activate with:  eval $(shot shell-hook)

# Use lumen, the library we built, to write a 128px thumbnail of photo.jpg
$ shot run lua -e "require('lumen').thumbnail('photo.jpg', 128)"
wrote photo_thumb.jpg (128px thumbnail of photo.jpg)
```

`lumen` is a small Lua module we will build in [Chapter 10](ch10-build.md). It wraps
ImageMagick's command-line tool, which means it depends on a C library with
dozens of native dependencies. The interesting part is not the Lua code itself,
but that our package manager handles the entire native dependency chain.
Where did `lumen` come from? We built it with moonshot's other headline command:

```console
$ shot build --output-dir ../channel
Building lumen 0.1.0 (build lua_0)
  → Installing 1 build dependencies…
  → Running build script `build.lua`
  → Packing 3 files…
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
language.  We use [Lua] (a lightweight scripting language commonly embedded in games and applications) as the target language because it's small and
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

[rattler]: https://github.com/conda/rattler
[repo]: https://github.com/prefix-dev/rattler-book
[Lua]: https://www.lua.org
[conda-forge]: https://conda-forge.org
