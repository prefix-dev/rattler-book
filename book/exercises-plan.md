# Exercises Plan

## Overview

24 programming exercises for chapters 3-10 of the rattler book. Each chapter gets 1 easy, 1 intermediate, and 1 hard exercise. Exercises extend the moonshot package manager, teaching rattler API usage as the primary goal and Rust as secondary.

## Infrastructure

### JJ Sandbox Skill

A project-level Claude Code skill at `.claude/skills/jj-excercise-sandbox.md` for manual/interactive exercise testing:

1. Create a new `jj workspace` if needed (lets take a max of 3).
1. Creates a new jj change from main: `jj new main -m "excercise <number>: <description>"`
2. Does the requested work
3. Runs `pixi run build` to verify compilation
4. Abandons the change with `jj abandon @` (unless told to keep)

### Verification Strategy

Lets use an agent team with this document as shared knowledge, we could even update this as we go.
Verify all 24 exercises using isolated the jj sandbox skill. Each exercise gets its own worktree so attempts don't interfere. Run in batches of 3 agents in parallel. Make sure to clean up after everything is done because rust compilation artifacts is big.

Each verification agent:
- Attempts the exercise following only the description and hints
- Runs `pixi run build` to check compilation
- Runs `pixi run shot <new-cmd>` where relevant
- Reports: feasibility, incorrect hints, suggested tips for readers

Local rattler checkout at `../rattler` available for verifying API hints. DO NOT ASSUME, VERIFY!

### Writing Style

All exercise text follows AGENTS.md prose rules:
- No em-dashes; use commas, semicolons, or periods instead
- No overused intensifiers ("crucial", "robust", "comprehensive", etc.)
- No filler adverbs, no elevated register ("use" not "utilize", etc.)
- Plain, direct language

---

## Chapter 3 - Init

**Rattler APIs covered:** `MatchSpec`, `ParseMatchSpecOptions`, `VirtualPackage::detect()`, `GenericVirtualPackage`, `Gateway`, `Channel`, `ChannelConfig`

### 3.1 Add a `requires-lua` Field (Easy)

Add a top-level `requires-lua` field to `moonshot.toml` (similar to `requires-python` in pyproject.toml). This field is more ergonomic than putting the Lua constraint in `[dependencies]` because it expresses the Lua version as a project-level requirement, not a regular dependency. Parse and validate it through `MatchSpec::from_str`. The `shot init` command gets a `--lua-version` flag to set it.

**Acceptance criteria:**
- `shot init --lua-version ">=5.1,<5.5"` writes `requires-lua = ">=5.1,<5.5"` to `[project]`
- `shot init --lua-version "!!!invalid"` fails with a parse error before creating any file
- Default (no flag) writes `requires-lua = ">=5.4"`
- `Manifest` struct has a `requires_lua: Option<String>` field that round-trips through TOML

**Hints:**
- `rattler_conda_types::MatchSpec::from_str(spec, ParseMatchSpecOptions::default())` for validation
- Modify `src/manifest.rs` (add field to `ProjectMetadata`) and `src/commands/init.rs` (add CLI flag)
- Pattern: see `Manifest::match_specs()` in `src/manifest.rs`

**Dependencies:** None

### 3.2 Detect and Record Virtual Packages (Intermediate)

At init time, detect the system's virtual packages using `VirtualPackage::detect()` and print them to stdout. Write a `[system]` section into the manifest with the detected values (e.g., `glibc = "2.31"` on Linux, `osx = "15.0"` on macOS). This gives users visibility into what their build host provides, which matters when the project later resolves dependencies (Ch6) or builds platform-specific packages (Ch10).

**Acceptance criteria:**
- `shot init` prints detected virtual packages (e.g., `Detected: __glibc=2.31, __archspec=x86_64`)
- Manifest contains `[system]` with key-value pairs for detected packages
- On macOS `__osx` is recorded; on Linux `__glibc` is recorded
- `[system]` is omitted from serialization when empty

**Hints:**
- `rattler_virtual_packages::VirtualPackage::detect(&VirtualPackageOverrides::default())`
- Convert each `VirtualPackage` to `GenericVirtualPackage` which has `.name` (`PackageName`) and `.version` (`Version`)
- Modify `src/manifest.rs` (add `system: HashMap<String, String>`) and `src/commands/init.rs`
- Pattern: see how `src/session.rs` already calls `VirtualPackage::detect`

**Dependencies:** None

### 3.3 Init with Gateway Validation (Hard)

Add a `--validate` flag to `shot init` that queries the configured channels to verify the Lua version constraint is satisfiable before writing the manifest. This requires constructing an HTTP client, creating a `Gateway`, and querying for a `MatchSpec` matching the `requires-lua` value. If no matching Lua packages exist in the channel, abort with a clear error.

**Acceptance criteria:**
- `shot init --validate` succeeds when `lua >=5.4` exists on conda-forge
- `shot init --validate --lua-version ">=99.0"` fails with "No Lua packages matching >=99.0 found in channels"
- Without `--validate`, init works offline as before
- The channels from `--channel` flags (or the default) are used for the query

**Hints:**
- Build an HTTP client using the pattern in `src/client.rs`
- `Gateway::builder().with_client(client).finish()` to create the gateway
- `Gateway::query(channels, [Platform::current(), Platform::NoArch], [matchspec])` to check
- Follow the gateway query pattern in `src/commands/search.rs`
- Modify `src/commands/init.rs`

**Dependencies:** Exercise 3.1 (uses the `requires-lua` field)

---

## Chapter 4 - Search

**Rattler APIs covered:** `PackageRecord` (build, version, depends, subdir), `Platform::from_str`, `Gateway::query`, `MatchSpec::from_str`, `PackageName`

### 4.1 Show All Versions with Build Strings (Easy)

Currently `shot search` deduplicates results to show only the latest version per package name. Add a `--all-versions` flag that displays every version found in the repodata. For each version, show the `build` string from `PackageRecord`, giving users visibility into how packages are built.

**Acceptance criteria:**
- `shot search lua --all-versions` shows multiple versions (e.g., 5.4.7, 5.4.6, 5.3.5) each with their build string
- Default behavior (without flag) is unchanged
- Output format: `lua    5.4.7    h5505292_0`

**Hints:**
- `PackageRecord::build` (String), `PackageRecord::version` (VersionWithSource, use `.to_string()`)
- Modify `src/commands/search.rs`, adjust the dedup/display logic

**Dependencies:** None

### 4.2 Display Package Dependencies from Repodata (Intermediate)

Add a `--deps` flag to `shot search` that prints the dependency list for each matching package. Access `PackageRecord::depends` (a `Vec<String>` of dependency specs) and display each dependency on its own indented line. Parse each dependency back through `MatchSpec::from_str` to validate it and show the structured name + version constraint.

**Acceptance criteria:**
- `shot search lua --deps` shows the latest version of `lua` with its dependencies listed below
- Each dependency is indented and shows name + constraint (e.g., `  libgcc-ng >=12`)
- All dependency strings parse through `MatchSpec::from_str` without error
- Packages with no dependencies show `(no dependencies)`

**Hints:**
- `PackageRecord::depends` is `Vec<String>`, each entry is a conda dependency spec
- `MatchSpec::from_str(dep_str, ParseMatchSpecOptions::default())` to parse
- Modify `src/commands/search.rs`

**Dependencies:** None

### 4.3 Reverse Dependency Search (Hard)

Implement `shot search --rdeps <package>` that finds all packages in the channel which depend on the given package. This requires fetching the full repodata (not just matching a single spec), iterating over every `PackageRecord`, parsing each entry in `depends` via `MatchSpec::from_str`, and checking if the dependency name matches the target package. Display the results grouped by depending package name.

**Acceptance criteria:**
- `shot search --rdeps lua` shows packages like `luarocks`, `busted`, etc. that have `lua` in their `depends`
- Results show the depending package name, version, and the specific constraint on the target (e.g., `luarocks 3.11.1 depends on lua >=5.1`)
- The gateway query uses a broad spec (e.g., `*`) to fetch all packages, then filters locally
- Performance is acceptable (scanning tens of thousands of records should complete in seconds)

**Hints:**
- Query the gateway with a wildcard or empty spec to get all repodata
- `PackageRecord::depends` contains strings like `"lua >=5.1"`, parse with `MatchSpec::from_str`
- `MatchSpec::name` gives the `Option<PackageName>` for matching against the target
- Modify `src/commands/search.rs`

**Dependencies:** None

---

## Chapter 5 - Add

**Rattler APIs covered:** `MatchSpec::from_str`, `ParseMatchSpecOptions`, `Gateway::query`, `PackageRecord::version`, `Version` ordering, `Platform::from_str`

### 5.1 Validate Specs Before Adding (Easy)

Currently `shot add` writes the package string directly to the manifest without checking if it's a valid spec. Add validation that parses each user-provided spec through `MatchSpec::from_str` before writing. If any spec is malformed, abort without modifying the manifest.

**Acceptance criteria:**
- `shot add lua` succeeds (valid name)
- `shot add "lua >=5.4"` succeeds (valid name + version)
- `shot add "!!!invalid"` fails with a parse error, manifest unchanged
- If adding multiple packages and one fails, none are added

**Hints:**
- `MatchSpec::from_str(spec_str, ParseMatchSpecOptions::default())`
- Modify `src/commands/add.rs`, add validation loop before the write loop
- Pattern: see `Manifest::match_specs()` in `src/manifest.rs`

**Dependencies:** None

### 5.2 Validate Package Exists in Channel Before Adding (Intermediate)

Make `shot add` query the repodata gateway by default to verify each package exists in the configured channels before adding it. If a package is not found, refuse to add it. Construct a `Session`, query with the parsed `MatchSpec`, and check that at least one matching record comes back. Add `--offline` to skip the check for users without network access.

**Acceptance criteria:**
- `shot add lua` queries conda-forge and succeeds (lua exists)
- `shot add nonexistent-package-xyz` fails with "Package not found in channels: ..."
- `shot add --offline lua` skips the gateway check and adds without validation
- The manifest's configured channels are used for the query

**Hints:**
- Create a `Session::new(project)` to get gateway access
- `Gateway::query(channels, [Platform::current(), Platform::NoArch], [spec]).recursive(false)`
- Check if returned repodata has any records
- Follow the gateway pattern in `src/commands/search.rs`
- Modify `src/commands/add.rs`

**Dependencies:** None

### 5.3 Platform-Specific Dependencies (Hard)

Implement `shot add --platform linux-64 lua` which adds the dependency to a platform-specific table `[platform-dependencies.linux-64]` instead of the global `[dependencies]`. This requires extending the `Manifest` struct with a `platform_dependencies: HashMap<String, HashMap<String, String>>` field, parsing the target platform with `Platform::from_str`, and optionally validating via the gateway for that specific platform.

**Acceptance criteria:**
- `shot add --platform linux-64 lua` writes to `[platform-dependencies.linux-64]`
- `shot add --platform linux-64` validates the package exists for linux-64 specifically (gateway is on by default)
- Without `--platform`, behavior is unchanged (adds to `[dependencies]`)
- Invalid platform strings produce a clear error
- Multiple `--platform` flags add to each platform section

**Hints:**
- `rattler_conda_types::Platform::from_str("linux-64")` to parse and validate
- `Gateway::query(channels, [target_platform, Platform::NoArch], specs)` for platform-specific query
- Modify `src/manifest.rs` (add `platform_dependencies` field with `#[serde(default, skip_serializing_if = "HashMap::is_empty")]`)
- Modify `src/commands/add.rs` (add `--platform` flag, route to correct table)

**Dependencies:** None (implements its own manifest change)

---

## Chapter 6 - Lock

**Rattler APIs covered:** `RepoDataRecord`, `PackageRecord` fields, `LockFile`, `read_lock_file`, `GenericVirtualPackage`, `VirtualPackage::detect`, `SolverTask`

### 6.1 Print Solve Solution Table (Easy)

After resolving, print a formatted table showing every package in the solution. For each `RepoDataRecord`, display: package name, version, build string, and subdir.

**Acceptance criteria:**
- `shot lock` prints a table like:
  ```
  lua           5.4.7    h5505292_0    linux-64
  libgcc-ng     14.2.0   h69a702a_2    linux-64
  ```
- Columns are aligned
- Count matches the "Solved N packages" message

**Hints:**
- `RepoDataRecord::package_record` contains `.name`, `.version`, `.build`, `.subdir`
- `PackageName::as_normalized()` for display
- Modify `src/commands/lock.rs`, add printing after `ensure_resolved`

**Dependencies:** None

### 6.2 Lock File Diff (Intermediate)

When re-locking (lock file already exists), compare the old and new solutions and print a diff. Read the old lock file before resolving, then compare package names and versions between old and new. Show added (+), removed (-), and upgraded/downgraded (~) packages.

**Acceptance criteria:**
- Adding a dependency then running `shot lock --force` shows `+ newpkg 1.0.0`
- Removing a dependency shows `- oldpkg 2.0.0`
- Version changes show `~ pkg 1.0.0 -> 1.1.0`
- No changes shows "Lock file unchanged"

**Hints:**
- `read_lock_file(lock_path, platform)` from `src/lock.rs` to read old solution
- Build `HashMap<PackageName, VersionWithSource>` for old and new, then diff
- `PackageName` implements `Eq + Hash`; `VersionWithSource` implements `Ord`
- Modify `src/commands/lock.rs`

**Dependencies:** None

### 6.3 Virtual Package Overrides via Manifest (Hard)

Add a `[virtual-packages]` table to `moonshot.toml` where users can override detected virtual packages for solving. This lets users target older systems (e.g., `__glibc = "2.17"` for manylinux2014 compatibility). Parse the table, construct `GenericVirtualPackage` values, and inject them into the `SolverTask` instead of auto-detected ones.

**Acceptance criteria:**
- Adding `[virtual-packages]` with `__glibc = "2.17"` to moonshot.toml makes the solver use glibc 2.17
- Multiple overrides in the table work: `__glibc = "2.17"` and `__cuda = "11.8"`
- Non-overridden virtual packages (e.g., `__unix`) are preserved from detection
- Invalid package names (missing `__` prefix) or unparseable versions produce clear errors
- `shot lock` reads the table and applies overrides before solving

**Hints:**
- Add `virtual_packages: HashMap<String, String>` to `Manifest` with `#[serde(default, skip_serializing_if = "HashMap::is_empty")]`
- `GenericVirtualPackage { name: PackageName::from_str("__glibc"), version: Version::from_str("2.17"), build_string: "0".into() }`
- `VirtualPackage::detect(...)` for defaults, then replace matching names with manifest overrides
- `SolverTask { virtual_packages, ... }` in `src/session.rs`
- Modify `src/manifest.rs`, `src/session.rs` (add override parameter to `resolve()`), and `src/commands/lock.rs`

**Dependencies:** None

---

## Chapter 7 - Install

**Rattler APIs covered:** `PrefixRecord::collect_from_prefix`, `PackageRecord` fields, `Session::install_packages`, `read_lock_file`

### 7.1 List Installed Packages (Easy)

Add a `shot list` command that reads the installed prefix and lists all packages. Use `PrefixRecord::collect_from_prefix` to discover installed packages, then display each one's name, version, and build string.

**Acceptance criteria:**
- `shot list` prints all installed packages sorted alphabetically
- If `.env/` does not exist, prints "No environment found. Run `shot install` first."
- Total count printed at the end

**Hints:**
- `PrefixRecord::collect_from_prefix::<PrefixRecord>(prefix_path)`
- `PrefixRecord` derefs to `PackageRecord` (`.name`, `.version`, `.build`)
- Create `src/commands/list.rs`, register in `src/commands/mod.rs` and `src/main.rs`

**Dependencies:** None

### 7.2 Dry-Run Installation (Intermediate)

Add a `--dry-run` flag to `shot install` that resolves dependencies and shows what would be installed without actually downloading or linking anything. Compare the resolved packages against what's already in the prefix (via `PrefixRecord::collect_from_prefix`) and report what would be added, updated, or unchanged.

**Acceptance criteria:**
- `shot install --dry-run` shows packages that would be installed with their versions and sizes
- Already-installed packages are listed as "unchanged" or "update from X to Y"
- No files are downloaded or written to the prefix
- Exit code 0 on success

**Hints:**
- Resolve via `Session::ensure_resolved(force)` to get the solution
- `PrefixRecord::collect_from_prefix(prefix)` to read what's already installed
- Compare by `PackageName` between resolved and installed sets
- `PackageRecord::size` gives the download size
- Modify `src/commands/install.rs`, short-circuit before `install_packages`

**Dependencies:** None

### 7.3 Reinstall Command (Hard)

Implement `shot reinstall` that removes the existing environment prefix and re-installs everything from the lock file. This forces a clean install, which is useful when the prefix is corrupted or when switching platforms. Read the lock file, remove the prefix directory, then run the full install pipeline. Add a `--relock` flag that also re-resolves before installing.

**Acceptance criteria:**
- `shot reinstall` removes `.env/`, reads the lock file, and installs all locked packages fresh
- If no lock file exists, it resolves first then installs
- `shot reinstall --relock` forces re-resolution before installing (equivalent to `shot lock --force && shot install`)
- Progress output shows the full install (downloading + linking)
- After reinstall, `shot list` shows the same packages as before
- `PrefixRecord::collect_from_prefix` confirms the prefix matches the lock file after reinstall

**Hints:**
- `std::fs::remove_dir_all(prefix)` to clear the prefix
- `read_lock_file(lock_path, platform)` from `src/lock.rs` to get locked packages
- `Session::install_packages(records)` to install
- `Session::ensure_resolved(force)` with `force=true` for `--relock`
- Create `src/commands/reinstall.rs` or add as a flag to `src/commands/install.rs`
- Register in `src/main.rs`

**Dependencies:** None

---

## Chapter 8 - Shell Hook

**Rattler APIs covered:** `Environment::activation_env`, `Activator`, `ActivationVariables`, `ShellEnum`

### 8.1 Show Activation Environment Variables (Easy)

Add a `--show-env` flag to `shot shell` that prints the environment variables activation would set, instead of the activation script. Use `Environment::activation_env()` and compare against `std::env::vars()` to show only changed variables.

**Acceptance criteria:**
- `shot shell --show-env` prints lines like `PATH=/path/to/env/bin:...`
- Only variables that differ from the current environment are shown
- Variables are sorted alphabetically
- Count of modified variables printed at the end

**Hints:**
- `Environment::activation_env()` in `src/environment.rs`
- `std::env::vars()` for current environment comparison
- Modify `src/commands/shell.rs` (add flag, call activation_env instead of activate_script)
- Note: `activation_env` is async; the shell command may need to become async

**Dependencies:** None

### 8.2 Generate Dotenv File from Activation (Intermediate)

Add `shot shell --dotenv [path]` that writes the activation environment to a `.env` file in dotenv format. This lets other tools (Docker, systemd, IDE run configs) consume the environment without shell-specific activation. Use the `Activator` to compute the full environment, diff against the current env, and write only the changed variables.

**Acceptance criteria:**
- `shot shell --dotenv` writes `.env` in the project root
- `shot shell --dotenv /tmp/my.env` writes to the specified path
- File format: `KEY=VALUE` per line, values quoted if they contain spaces
- Only activation-added/changed variables are included (not the full inherited environment)

**Hints:**
- `Environment::activation_env()` returns `HashMap<String, String>`
- Compare with `std::env::vars()` to find the diff
- Dotenv format: `KEY="value with spaces"` or `KEY=simple_value`
- Modify `src/commands/shell.rs`

**Dependencies:** None

### 8.3 Stacked Environment Activation (Hard)

Implement `shot shell --stack /other/env` that generates an activation script layering a second environment on top of the currently active one. Construct `ActivationVariables` from the already-activated environment state, then run the `Activator` for the stacked prefix. The result should have both envs on PATH in the correct order.

**Acceptance criteria:**
- `eval $(shot shell)` then `eval $(shot shell --stack /other/env)` puts both envs on PATH
- Stacked env's `bin/` appears before the base env's `bin/`
- `CONDA_PREFIX` reflects the top-of-stack environment
- A `MOONSHOT_STACK_DEPTH` env var tracks nesting level

**Hints:**
- `ActivationVariables { conda_prefix: Some(base_prefix), path: current_path_vec }` constructed from current state
- `Activator::from_path(stacked_prefix, shell, platform)`
- `activator.activation(vars)` passes the existing activation state
- Modify `src/environment.rs` and `src/commands/shell.rs`

**Dependencies:** None

---

## Chapter 9 - Run

**Rattler APIs covered:** `Environment::activation_env`, `Activator::run_activation`, `PrefixRecord::collect_from_prefix`, `Session::ensure_resolved`

### 9.1 Lua REPL with Auto-Configured Paths (Easy)

Add `shot repl` that launches the Lua REPL in the activated environment with `LUA_PATH` and `LUA_CPATH` auto-configured to point to the correct directories in the prefix. This saves users from manually setting Lua's module search paths.

**Acceptance criteria:**
- `shot repl` launches `lua` (interactive) with the activated environment
- `LUA_PATH` includes `<prefix>/share/lua/5.4/?.lua` and `<prefix>/share/lua/5.4/?/init.lua`
- `LUA_CPATH` includes `<prefix>/lib/lua/5.4/?.so`
- The Lua version in the paths matches what's installed (detected from the prefix)
- If `lua` is not installed, error message says "No Lua interpreter found. Run `shot install` first."

**Hints:**
- `Environment::activation_env()` for the base environment
- Detect Lua version by checking `<prefix>/bin/lua -v` output or scanning `<prefix>/share/lua/`
- `tokio::process::Command::new("lua").envs(&env).spawn()` to launch
- Modify `src/commands/run.rs` or create `src/commands/repl.rs`

**Dependencies:** None

### 9.2 Run with Extra Environment Variables (Intermediate)

Add `--env KEY=VALUE` flags to `shot run` that inject extra environment variables on top of the activation environment. These are applied after activation, so they can override activation-set values.

**Acceptance criteria:**
- `shot run --env MY_VAR=hello lua -e "print(os.getenv('MY_VAR'))"` prints `hello`
- Multiple `--env` flags work: `--env A=1 --env B=2`
- Invalid format (no `=`) produces a clear error
- Extra vars override activation vars if they conflict

**Hints:**
- `Environment::activation_env()` returns `HashMap<String, String>`, just `.insert()` the extras
- Parse `KEY=VALUE` by splitting on the first `=`
- Modify `src/commands/run.rs` (add `--env` flag to `Args`)

**Dependencies:** None

### 9.3 Auto-Install Before Run (Hard)

Make `shot run` check lock freshness and prefix existence before executing. If the lock is stale or the prefix is missing/incomplete, automatically resolve and install. Check installed packages against the lock file using `PrefixRecord::collect_from_prefix` to detect if packages were manually deleted.

**Acceptance criteria:**
- `shot run lua -v` on a fresh project (no `.env/`) automatically resolves, installs, then runs
- If the lock is fresh and prefix is complete, no resolve/install happens (fast path)
- If a package is deleted from `.env/`, staleness check detects it and re-installs
- `--no-auto-install` skips the check (errors if env is missing)
- Resolve/install output appears before the command output

**Hints:**
- `Project::is_lock_fresh()` checks lock vs manifest mtime
- `PrefixRecord::collect_from_prefix::<PrefixRecord>(prefix)` for installed packages
- `read_lock_file(lock_path, platform)` for expected packages
- `Session::ensure_resolved()` + `Session::install_packages()` for the full pipeline
- Modify `src/commands/run.rs`

**Dependencies:** None

---

## Chapter 10 - Build

**Rattler APIs covered:** `rattler_package_streaming::seek::stream_conda_info`, `IndexJson`, `PathsJson`, `rattler_index::index_fs`, `write_conda_package`

### 10.1 Inspect Package Contents (Easy)

Add `shot build --inspect <file.conda>` that reads an existing `.conda` package and displays its metadata and file listing. Use `rattler_package_streaming` to read the archive, extract `info/index.json` for metadata and `info/paths.json` for the file list.

**Acceptance criteria:**
- `shot build --inspect output/noarch/mypkg-0.1.0-lua_0.conda` prints name, version, build, dependencies
- A file listing shows all files with their sizes
- Invalid files produce clear errors

**Hints:**
- `rattler_package_streaming::seek::stream_conda_info(reader)` streams the info section as a tar archive
- `IndexJson` and `PathsJson` are in `rattler_conda_types::package`
- Deserialize from JSON found in the tar entries
- Modify `src/commands/build.rs` (add `--inspect` as alternative execution path)

**Dependencies:** None

### 10.2 Extended Package Metadata (Intermediate)

Include `license` and `description` from the manifest in the built package's `IndexJson`, and write an `about.json` file to the package's info directory. Add optional `home` and `dev_url` fields to the manifest's `[project]` section.

**Acceptance criteria:**
- Built package's `info/index.json` has the `license` field populated
- An `info/about.json` file exists with description, license, home, dev_url
- Missing optional fields are omitted (not null)
- Verifiable with `--inspect` from exercise 10.1 (or by extracting the .conda)

**Hints:**
- `IndexJson::license` field already mapped in `src/commands/build.rs`
- Add `home: Option<String>`, `dev_url: Option<String>` to `ProjectMetadata` in `src/manifest.rs`
- Write `about.json` in the `write_package_metadata` section of `src/commands/build.rs`
- The `info/` directory is created around line 152 of `build.rs`

**Dependencies:** None

### 10.3 Build Variants (Hard)

Implement `shot build --variant KEY=VALUE` that produces different packages from the same source with different configurations. Each variant combination gets a unique build string (e.g., `lua54_0` vs `lua51_0`). Variant keys are injected as environment variables during the build script and encoded in the build string and `IndexJson`. Building with `--variant lua=5.4` and `--variant lua=5.1` produces two separate `.conda` packages.

**Acceptance criteria:**
- `shot build --variant lua=5.4` produces a package with build string containing `lua54`
- `shot build --variant lua=5.1` produces a different package with `lua51` in the build string
- Multiple variants: `--variant lua=5.4 --variant opt=release` produces `lua54_release_0`
- Variant keys are available as env vars during build (e.g., `VARIANT_LUA=5.4`)
- Both packages can coexist in the output directory with separate filenames
- `rattler_index::index_fs` indexes all variant packages correctly

**Hints:**
- Modify `Manifest::build_string()` in `src/manifest.rs` to accept variant info
- Variants encode into the build string by joining key-value pairs (sanitize: replace `.` with `_`)
- Pass variants as env vars to the `LuaBuildBackend` via the `Command` environment
- `write_conda_package` uses the build string for the filename
- `index_fs` indexes everything in the output dir, so multiple packages work automatically
- Modify `src/commands/build.rs` and `src/manifest.rs`

**Dependencies:** None
