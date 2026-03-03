# Deep Dive: The resolvo SAT Solver

The dependency solver is the most mathematically interesting component of any
package manager.  In this chapter we explore how resolvo works — from the
connection to SAT to the specific optimizations that make it fast in practice.

## Dependency solving as SAT

**SAT** (Boolean satisfiability) asks: given a boolean formula, is there an
assignment of variables to true/false that makes it true?

Dependency solving reduces to SAT by encoding each package version as a boolean
variable:

```
lua_5_4_7 = true means "install lua 5.4.7"
lua_5_4_6 = true means "install lua 5.4.6"
```

Each constraint becomes a clause:

- "Install exactly one version of lua":
  `lua_5_4_7 XOR lua_5_4_6 XOR ...`

- "If `luarocks_3_11` is installed, `lua >= 5.1` must be installed":
  `NOT luarocks_3_11 OR lua_5_4_7 OR lua_5_4_6 OR lua_5_3_6 OR ...`

- "The user requested `lua >= 5.4`":
  `lua_5_4_7 OR lua_5_4_6` (but not 5.3 or older)

A SAT solver finds an assignment that satisfies all clauses, or reports UNSAT if
none exists.

## DPLL and CDCL: the solver algorithms

Modern SAT solvers use **CDCL** (Conflict-Driven Clause Learning), an evolution
of the older DPLL algorithm.

### DPLL in brief

DPLL works by:
1. **Unit propagation**: if a clause has only one unset variable, that variable
   must be set to make the clause true.  This is cheap and often resolves many
   variables immediately.
2. **Choose**: pick an unset variable and assume a value (e.g., set `lua_5_4_7 = true`).
3. **Recurse**: run unit propagation with the new assumption.
4. **Backtrack**: if a conflict is found, undo the last assumption and try the
   opposite.

### CDCL: learning from conflicts

CDCL improves DPLL by analyzing *why* a conflict occurred and adding a new clause
that prevents reaching the same dead end via a different path.

```
Conflict: tried lua 5.4.7, failed because luarocks needs json-lib <2.0
         but lua 5.4.7 doesn't need json-lib.  The conflict comes from
         legacy-plugin requiring json-lib <2.0 AND json-lib 2.1 being
         required by web-server 2.0.

Learned clause: NOT (web-server_2_0 AND legacy-plugin)
```

The learned clause is added to the formula and persists for the rest of the
search.  This can dramatically prune the search space.

## resolvo's design

[resolvo] is a pure-Rust CDCL solver designed specifically for package
dependency solving.  It was developed by the pixi team for use in conda-style
dependency resolution.

[resolvo]: https://github.com/mamba-org/resolvo

Key design decisions:

### Lazy candidate generation

In traditional SAT, all clauses are known upfront.  In package solving, the
"clauses" are the dependency constraints of each package — and you might not
know what those constraints are until you decide to install a package.

resolvo's `DependencyProvider` trait allows lazy fetching:

```rust
pub trait DependencyProvider {
    // Fetch what versions of a package are available
    fn get_candidates(&self, name: NameId) -> Option<Candidates>;

    // Fetch the dependencies of a specific version
    fn get_dependencies(&self, solvable: SolvableId) -> Dependencies;
}
```

The solver calls `get_dependencies` only for the packages it's actually
considering.  In a large channel like conda-forge, this means you never load
dependency data for the thousands of packages you don't need.

### Arena allocators

resolvo uses **arena allocators** for performance.  Instead of allocating each
package record on the heap separately (which involves many small `malloc` calls
and pointer chasing), records are packed into a single large array:

```rust
pub struct Arena<T> {
    data: Vec<T>,
}

impl<T> Arena<T> {
    pub fn alloc(&mut self, item: T) -> Id<T> {
        let id = self.data.len();
        self.data.push(item);
        Id(id as u32)
    }

    pub fn get(&self, id: Id<T>) -> &T {
        &self.data[id.0 as usize]
    }
}
```

`Id<T>` is a `u32` index, not a pointer.  This has several advantages:
- Cache-friendly: related items are adjacent in memory.
- Smaller: a `u32` index is 4 bytes; a pointer is 8 bytes.
- No lifetime annotations: indices don't borrow.
- Fast: integer comparison vs pointer dereferencing.

### The watch list optimization

A key unit propagation optimization is the **watch list** (also used in the
famous MiniSat solver).

For each clause `(A OR B OR C OR ...)`, we "watch" two of its literals.  When a
watched literal becomes false, we try to find another literal to watch.  If we
can't (because all other literals are also false), we've found a unit clause —
the remaining watched literal must be true.

This avoids scanning all clauses on every assignment change.  Instead we maintain
a list of clauses watching each literal, and only process those clauses when that
literal changes.

## Conda-specific heuristics

General SAT solvers don't know about package manager conventions.  resolvo adds
conda-specific scoring:

### Preferring higher versions

When choosing which variable to set next, the solver prefers:
1. The version directly requested by the user (highest version that satisfies the
   spec).
2. Currently-installed versions (avoid unnecessary changes).
3. Higher versions over lower ones.
4. Earlier channels over later channels.

These preferences are encoded as "decisions" that the solver makes before
backtracking.  If a preference leads to a conflict, the solver backtracks and
tries the next preference.

### Explaining conflicts

When the solver can't find a solution, it generates a human-readable explanation.
This is one of resolvo's distinctive features:

```
The following packages are incompatible:
  • luarocks 3.11 requires json-lib >=2.0
  • legacy-plugin 1.0 requires json-lib <2.0
  Therefore json-lib cannot be installed.

  And because you requested both luarocks and legacy-plugin,
  no solution exists.
```

Generating this explanation is non-trivial — the solver must trace back through
its conflict graph to find the minimal set of incompatibilities.

## The conda scoring model

rattler's interface to resolvo (`rattler_solve::resolvo`) translates the conda
preference model into resolvo's scoring system.

The key insight is that conda uses a **multi-objective** ordering:

```
1. Maximize version of user-requested packages
2. Minimize number of changes from locked packages
3. Maximize version of unlocked packages
4. Maximize build number (for the same version)
5. Prefer packages from earlier channels
```

These objectives are combined into a single total ordering over candidate sets.
The resolvo solver uses this ordering to guide its search.

## Practical performance

For typical conda environments (10-50 packages), resolvo solves in milliseconds.
For large environments (hundreds of packages), it can take a few seconds — which
is why we show a spinner.

The bottleneck is usually not the SAT solving itself but the repodata loading —
fetching and parsing package metadata.  This is why the Gateway's sparse/sharded
format is so important: it avoids loading millions of records for the vast
majority of conda-forge packages that aren't relevant to your request.

## libsolv: the alternative backend

rattler also ships a binding to `libsolv`, a C library used by older conda tools
(conda, mamba).  You can select it via feature flags:

```toml
rattler_solve = { version = "0.28", features = ["resolvo", "libsolv_c"] }
```

`resolvo` is the default and recommended backend.  It's faster, produces better
error messages, and doesn't require a C compiler.

The two backends share the same `SolverImpl` trait, so switching is a
one-line change.

## Summary

- Dependency solving reduces to SAT.
- CDCL improves on DPLL by learning clauses from conflicts.
- resolvo uses lazy dependency loading, arena allocators, and watch lists for
  efficiency.
- Conda-specific heuristics encode version preferences and minimize changes.
- Conflict explanations trace the incompatibility graph for human-readable errors.
- resolvo is faster than libsolv and produces better diagnostics.
