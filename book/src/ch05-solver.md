# Chapter 5: Solving Dependencies

We have a list of packages the user asked for and a catalog of all available
package versions.  Now we need to find a *consistent set*, versions that
satisfy all constraints simultaneously.  This is the solver's job.

As discussed in Chapter 1, conda enforces the "exactly one version per package" constraint. This single rule is what makes solving NP-hard, and it is the reason we need a SAT-based solver instead of a simple graph traversal.

## Why solving is hard

Imagine you ask for two packages:

```toml
[dependencies]
web-server = "*"
json-lib   = "*"
```

And the catalog says:
- `web-server 2.0` depends on `json-lib >=2.0`
- `web-server 1.0` depends on `json-lib >=1.0,<2.0`
- `json-lib 2.1` (latest)
- `json-lib 1.9`

The solver tries `web-server 2.0` + `json-lib 2.1`.  That works.  Done.

But now add another constraint:
```toml
legacy-plugin = "*"  # depends on json-lib <2.0
```

Now `web-server 2.0` is incompatible with `legacy-plugin`.  The solver has to
backtrack and try `web-server 1.0` + `json-lib 1.9` + `legacy-plugin`.

In the general case, dependency solving is equivalent to
[SAT] (Boolean satisfiability), which is NP-hard.  In practice, real package
ecosystems have structure that makes good heuristics very effective.

[SAT]: https://en.wikipedia.org/wiki/Boolean_satisfiability_problem

## Virtual packages

Virtual packages model the host system as if it were a package. Instead of special-casing "requires glibc 2.17" as a platform check, the solver treats `__glibc` as a regular dependency that happens to be provided by the OS. This lets package authors express system requirements using the same constraint syntax they use for library dependencies.

Before calling the solver, we detect what the host system provides:

``` {.rust #install-virtual-packages}
    let virtual_packages: Vec<GenericVirtualPackage> =
        rattler_virtual_packages::VirtualPackage::detect(
            &rattler_virtual_packages::VirtualPackageOverrides::default(),
        )
        .into_diagnostic()
        .context("detecting virtual packages")?
        .into_iter()
        .map(|v| v.into())
        .collect();
```

This probes the system for things like:
- `__linux` — whether this is a Linux system
- `__glibc =2.38` — the installed glibc version
- `__cuda =12.3` — the CUDA toolkit version (if any)
- `__osx =14.4` — macOS version

Packages can list these as dependencies.  A CUDA-accelerated package might say
`__cuda >=11.0`; the solver will refuse to install it on a machine without CUDA.

`GenericVirtualPackage` is a simpler wrapper around `VirtualPackage` that
stores the name and version as strings, which is what the solver expects.

## Reading the existing installation

We scan the prefix's `conda-meta/` directory to find out what is already installed.

``` {.rust #install-read-installed}
    let installed_packages =
        PrefixRecord::collect_from_prefix::<PrefixRecord>(&prefix).into_diagnostic()?;
```

`PrefixRecord` is rattler's representation of a package that's already installed.
When rattler installs a package, it writes a JSON file to
`<prefix>/conda-meta/<name>-<version>-<build>.json` describing what was
installed.  `collect_from_prefix` reads all of those files.

We pass the installed packages to the solver as **locked packages**: versions the
solver should prefer to keep if possible.

!!! warning "Why locking matters"

    Without locking, every `luapkg install` could silently upgrade transitive
    dependencies even when the manifest hasn't changed. That kind of drift is a
    common source of "it worked yesterday" bugs. Locking gives you environmental
    stability: the solver only changes what it must to satisfy new or modified
    constraints.

## Building the solver task

We assemble the installed packages, virtual packages, specs, and repodata into a single `SolverTask`.

``` {.rust #install-solver-task}
    let locked = installed_packages
        .iter()
        .map(|r| r.repodata_record.clone())
        .collect::<Vec<_>>();

    let solver_task = SolverTask {
        locked_packages: locked,
        virtual_packages,
        specs: specs.clone(),
        ..SolverTask::from_iter(&repo_data)
    };
```

`SolverTask::from_iter(&repo_data)` builds the task's `available_packages` field
from our repodata.  We override the remaining fields with our specs, locked
packages, and virtual packages.

The complete `SolverTask` contains:

| Field | Description |
|---|---|
| `available_packages` | All packages the solver may choose from |
| `specs` | What the user requested |
| `locked_packages` | Currently installed (prefer to keep) |
| `virtual_packages` | Host system capabilities |
| `pinned_packages` | Packages that must stay at a specific version |

!!! tip "Locked vs pinned"

    The difference between locked and pinned is important: locked packages are a
    *preference* that the solver may override if constraints demand it, while
    pinned packages are a *hard constraint* that the solver must satisfy or
    report as a conflict.

## Running the solver

rattler ships two solver backends: `resolvo` (pure Rust, the default, used by pixi) and `libsolv_c` (a C binding to libsolv, used by older conda tooling).  We use resolvo throughout this book.

``` {.rust #install-solve}
    let start_solve = Instant::now();
    let solution: Vec<RepoDataRecord> = with_spinner_sync("Solving", || {
        resolvo::Solver.solve(solver_task)
    })
    .into_diagnostic()
    .context("solving dependencies")?
    .records;
```

`resolvo::Solver.solve(solver_task)` is synchronous and CPU-bound.  We run it in
`with_spinner_sync`, a version of our spinner helper that works with synchronous
closures:

``` {.rust #with-spinner-sync}
pub fn with_spinner_sync<T, F: FnOnce() -> T>(msg: impl Into<Cow<'static, str>>, f: F) -> T {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = f();
    pb.finish_and_clear();
    result
}
```

## What the solver returns

`resolvo::Solver.solve(...)` returns a `SolveResult` which contains a `.records`
field: a `Vec<RepoDataRecord>`.  Each record describes exactly one package
version to install, including its download URL.

The solution is guaranteed to be consistent: every dependency constraint of every
package in the solution is satisfied.  If no consistent solution exists, the
solver returns an error with an explanation of the conflict.

## Resolver strategy: how conda sorting works

The solver needs a way to choose between `lua 5.4.7` and `lua 5.4.6` when both
satisfy `>=5.4`.  The conda convention is:

1. Prefer **higher versions** of directly-requested packages.
2. Prefer **locked** (currently installed) versions of transitive dependencies.
3. Among unlocked, prefer **higher build numbers** (more recent builds of the
   same version).
4. Prefer packages from channels listed **earlier** in the channel list.

This biases the solver toward fresh versions for things you asked for, while
keeping the rest of your environment stable.  resolvo implements these priorities
through a scoring system, not a separate post-processing step. Without the "prefer locked for transitive deps" rule, adding one new package could cascade upgrades across your entire environment.

## Printing progress

After solving, we print a summary of how many packages were selected and how long it took.

``` {.rust #install-solve-progress}
    println!(
        "  Solved {} packages in {:.1}s",
        console::style(solution.len()).cyan(),
        start_solve.elapsed().as_secs_f64()
    );
```

`{:.1}` formats a float to one decimal place.  `start_solve.elapsed()` returns
a `std::time::Duration`; `.as_secs_f64()` converts it to seconds as a `f64`.

## Summary

- Dependency solving is a SAT problem; rattler uses the `resolvo` solver.
- Virtual packages represent host-system capabilities (glibc, CUDA, etc.).
- `PrefixRecord::collect_from_prefix` reads what's currently installed.
- The `SolverTask` bundles available packages, specs, locked packages, and
  virtual packages.
- The solver returns an exact list of `RepoDataRecord`s, one per package to
  install.

In the next chapter we take the solver's output and install the packages.
