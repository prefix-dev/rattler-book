# Chapter 5: Solving Dependencies

We have a list of packages the user asked for and a catalog of all available
package versions.  Now we need to find a *consistent set* — versions that
satisfy all constraints simultaneously.  This is the solver's job.

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

The solver tries `web-server 2.0` + `json-lib 2.1` — that works.  Done.

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

Before calling the solver, we detect what the host system provides:

```rust
let virtual_packages: Vec<GenericVirtualPackage> =
    rattler_virtual_packages::VirtualPackage::detect(
        &rattler_virtual_packages::VirtualPackageOverrides::default(),
    )
    .into_diagnostic()?
    .into_iter()
    .map(GenericVirtualPackage::from)
    .collect();
```

This probes the system for things like:
- `__linux` — whether this is a Linux system
- `__glibc =2.38` — the installed glibc version
- `__cuda =12.3` — the CUDA toolkit version (if any)
- `__osx =14.4` — macOS version

Packages can list these as dependencies.  A CUDA-accelerated package might say
`__cuda >=11.0` — the solver will refuse to install it on a machine without CUDA.

`GenericVirtualPackage` is a simpler wrapper around `VirtualPackage` that
stores the name and version as strings, which is what the solver expects.

### Rust concept: `.into_iter().map(...).collect()`

This is the Rust iterator pattern in its canonical form:

```rust
some_vec
    .into_iter()                // consume Vec, produce Iterator
    .map(GenericVirtualPackage::from) // transform each element
    .collect()                  // gather back into Vec
```

`into_iter()` consumes the vector, giving ownership of each element to the
closure.  This contrasts with `.iter()`, which borrows, and `.iter_mut()`, which
mutably borrows.

`GenericVirtualPackage::from` is a method reference — instead of writing
`|v| GenericVirtualPackage::from(v)`, we pass the function directly.  This works
when the `From` trait is implemented.

## Reading the existing installation

```rust
let installed_packages =
    PrefixRecord::collect_from_prefix::<PrefixRecord>(&prefix)
        .into_diagnostic()?;
```

`PrefixRecord` is rattler's representation of a package that's already installed.
When rattler installs a package, it writes a JSON file to
`<prefix>/conda-meta/<name>-<version>-<build>.json` describing what was
installed.  `collect_from_prefix` reads all of those files.

We pass the installed packages to the solver as **locked packages**: versions the
solver should prefer to keep if possible.  This makes re-running `luapkg install`
idempotent — if nothing changed in the manifest, the solver returns the same
solution.

## Building the solver task

```rust
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
from our repodata.  The `..SolverTask::from_iter(...)` syntax is **struct update
syntax**: it fills in all fields from the base expression, then overrides the
explicitly-named ones.

The complete `SolverTask` contains:

| Field | Description |
|---|---|
| `available_packages` | All packages the solver may choose from |
| `specs` | What the user requested |
| `locked_packages` | Currently installed (prefer to keep) |
| `virtual_packages` | Host system capabilities |
| `pinned_packages` | Packages that must stay at a specific version |

## Running the solver

```rust
let solution: Vec<RepoDataRecord> = with_spinner_sync("Solving", || {
    resolvo::Solver.solve(solver_task)
})
.into_diagnostic()
.context("solving dependencies")?
.records;
```

`resolvo::Solver.solve(solver_task)` is synchronous and CPU-bound.  We run it in
`with_spinner_sync` — a version of our spinner helper that works with synchronous
closures:

```rust
pub fn with_spinner_sync<T, F: FnOnce() -> T>(
    msg: impl Into<Cow<'static, str>>,
    f: F,
) -> T {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = f();
    pb.finish_and_clear();
    result
}
```

Note that we *don't* use `spawn_blocking` here even though this is synchronous
code inside an async context.  For short operations (solver typically finishes in
milliseconds) it's fine to run directly.  For long-running synchronous work
(parsing huge files, hashing data) you'd use `tokio::task::spawn_blocking` to
avoid blocking the async scheduler.

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
through a scoring system, not a separate post-processing step.

## Rust concept: `FnOnce` and closures

```rust
with_spinner_sync("Solving", || {
    resolvo::Solver.solve(solver_task)
})
```

The `||` starts a closure with no arguments.  The closure *captures* `solver_task`
from the surrounding scope.  The type bound `F: FnOnce() -> T` means the closure
can be called exactly once — appropriate here since we only solve once.

Rust has three closure traits:
- `FnOnce`: can be called once, may consume captured variables
- `FnMut`: can be called multiple times with mutable access to captures
- `Fn`: can be called multiple times with shared access

The compiler automatically picks the most restrictive trait that still works for
what the closure does.  Since `solver_task` is moved into the closure and
consumed by `.solve()`, the compiler uses `FnOnce`.

## Printing progress

```rust
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
- The solver returns an exact list of `RepoDataRecord`s — one per package to
  install.
- `FnOnce` is the closure trait for closures that may consume their captures.

In the next chapter we take the solver's output and actually install the packages.
