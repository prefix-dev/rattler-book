# Why Build on Conda?

## Wait, isn't conda just a Python thing?

Most people would forgive you for thinking so. Anaconda and conda were
definitely targeted at Python first. But Python's adoption in machine learning
and data science meant that packages quickly needed CUDA, BLAS, LAPACK (linear
algebra libraries), Fortran runtimes, and other native libraries. To ship NumPy you also need to ship a
Fortran runtime. To ship TensorFlow you need the CUDA runtime libraries. Once you are packaging
those, you are packaging for any language.

That cross-language requirement is what made conda suitable beyond data science
too. Robotics ecosystems like [RoboStack] adopted it for the same reason: real-world
projects mix C++, Python, and system libraries, and they need a single tool
that handles all of them.

[conda-forge] specifically is focused on providing compiled packages across a
range of language ecosystems, targeting a large number of platforms. In that way
it is more comparable to a system package collection like [Debian's archives][debian] or
[Nixpkgs] than to a language-specific registry like [PyPI] or [crates.io].

[debian]: https://www.debian.org/distrib/packages
[Nixpkgs]: https://github.com/NixOS/nixpkgs
[PyPI]: https://pypi.org
[crates.io]: https://crates.io

[RoboStack]: https://robostack.github.io/


## What does it provide?

Here's why you might want to choose conda as the foundation:

- **Existing packages.** [conda-forge] has thousands of packages across Python,
  R, C++, Fortran, etc. Your language's packages can depend on native libraries
  that are already packaged on conda-forge.
- **Binary distribution.** Packages ship as prebuilt binaries per platform. There is no
  compilation on the user's machine.
- **Isolated environments.** Environments can be isolated from each other,
  think of Python's `.venv` or a separate Nix profile, but with more native dependency types.
- **Mature tooling.** [rattler] provides a solver, installer, networking stack, and
  shell activation in reusable Rust crates.

One tradeoff to be aware of: conda allows only **one version per package** in an
environment. The solver must find a single compatible set of versions, which is a
harder problem than allowing duplicates. Other ecosystems avoid this entirely: Go
and Nix let multiple versions coexist, while Arch's pacman keeps only one version
per package in the repository so there is nothing to solve.

## Building on top of native dependencies

If your language has C bindings or native extensions, this is where conda
helps most. Most language package managers (with some exceptions) do not include
native libraries. They assume the OS provides them, or they ask the user to
install them separately. The difference is noticeable. No more hunting for
the right `-dev` package or debugging linker errors.

If you have ever had `pip install` fail because a C library was missing or the
wrong version, this is the problem conda solves.

As an example, Python discourages sdists (source distributions) for packages with C extensions, mainly
because setting up the build environment to compile correctly on someone's
machine is often painful. You might be missing native dependencies, or have the
wrong compiler version. With conda the libraries are already there, prebuilt
and version-pinned.

We've started proving this with pixi build. For example, [python/cpython](https://github.com/python/cpython/blob/main/Tools/pixi-packages/default/pixi.toml)
has a `pixi.toml` that lets you install it globally with:

```
pixi global install --git git@github.com:python/cpython.git python
```

/// margin-note
This also works inside a pixi project using `pixi add`.
///

That single command will:

1. Check out the correct git source.
2. Set up compilers, native dependencies, and build tools.
3. Run the isolated compilation.
4. Install it into a globally accessible location.

The same repository offers variant builds (like AddressSanitizer) in separate
directories. The fact that conda makes all of this possible means you can build
upon existing C libraries, or even let your package manager compile native
extensions from source.

## Conda-native vs using conda

Before we start building, there is a design decision worth thinking about.
There are two broad strategies for mixing conda with a language ecosystem.

**Conda-native** means repackaging your language's libraries as `.conda`
packages. The build script calls the language's own build tool (`gem build`,
`python -m build`, `luarocks make`) and installs the result into `$PREFIX`.
Everything goes through one solver, one lock file, one install. The solver sees
the full dependency graph: native libraries and language packages together. This
is what moonshot does for Lua, and what conda-forge does for Python, R, Fortran,
C++/C, etc.

Conda-native works well when:

- The language produces stable binary artifacts (interpreted, bytecode, or
  stable C ABI, meaning the binary interface stays compatible across compiler
  versions). Rust, for example, is hard to redistribute as binaries because it
  has no stable ABI: prebuilt `.rlib` files only work with the exact compiler
  version that produced them, so every consumer would need to rebuild from source.
- The ecosystem is small enough to repackage, or doing so is worth the effort.
  Automation helps: [RoboStack] uses [Vinca](https://github.com/RoboStack/vinca)
  to generate conda recipes from ROS package metadata automatically.

**Using conda** means using conda only for the native dependency graph. The
language's packages stay in their own format (crates, npm packages, gems). Conda
provides the native libraries (OpenSSL, libxml2, zlib) in the environment, and
the language's build tool links against them at compile time.

This lets you use an existing package manager like [pixi], but you lose some
benefits. You probably need to maintain two lock files, for example.

The team at [prefix.dev](https://prefix.dev/) and many others use this approach for their Rust projects. We
install `rust`, `openssl`, `clang` from conda-forge but use cargo for Rust
dependency management.

### This book

This book explores the conda-native direction, because frankly it is the more
interesting use case for developing a custom package manager. If you want to
*use* conda-forge rather than build a new tool on top of it, you might want to
use an existing package manager. For example, [Mojo by Modular](https://docs.modular.com/pixi/) does exactly that.

[conda-forge]: https://conda-forge.org
[rattler]: https://github.com/conda/rattler
[pixi]: https://pixi.sh
