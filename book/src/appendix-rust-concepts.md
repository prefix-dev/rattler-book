# Appendix: Rust Concepts Reference

This appendix collects the Rust concepts introduced throughout the book in one
place for quick reference.  Each section links to where the concept was first
used.

---

## Ownership, borrowing, and lifetimes

Rust's type system tracks *who owns* each piece of data and *how long* references
to it are valid.

- **Owned value**: `String`, `Vec<T>`, `PathBuf` — the value is on the heap;
  dropping it frees the memory.
- **Borrowed reference**: `&str`, `&[T]`, `&Path` — a temporary view into data
  owned elsewhere; can't outlive the owner.
- **Mutable reference**: `&mut T` — exclusive access to mutate; only one
  mutable reference may exist at a time.

The compiler enforces these rules at compile time with zero runtime cost.

---

## `Result<T, E>` and `?`

```rust
fn read_file(path: &Path) -> Result<String, io::Error> {
    let content = std::fs::read_to_string(path)?;
    Ok(content)
}
```

`?` on a `Result`:
- If `Ok(v)`, unwraps to `v`.
- If `Err(e)`, returns `Err(e.into())` from the current function.

The `.into()` conversion means `?` can convert between compatible error types
(anything that implements `From<E>`).

---

## `Option<T>`

```rust
let name: Option<String> = args.name;

let s: String = name
    .unwrap_or_else(|| "default".to_string());
```

- `Some(value)`: a value is present.
- `None`: no value.
- `.unwrap_or_else(closure)`: use the closure's result when `None`.
- `.and_then(closure)`: chain operations that might return `None`.
- `if let Some(x) = opt { ... }`: pattern-match on an `Option`.

---

## Derive macros

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Parser)]
struct Foo { ... }
```

Derive macros auto-generate trait implementations:
- `Debug`: enables `{:?}` formatting.
- `Clone`: enables `.clone()`.
- `Serialize`/`Deserialize`: enables serde (de)serialization.
- `Parser`: Clap CLI parsing.

---

## Enums and pattern matching

```rust
enum Command {
    Init(InitArgs),
    Add(AddArgs),
    Install(InstallArgs),
}

match command {
    Command::Init(args) => init(args).await,
    Command::Add(args)  => add(args).await,
    Command::Install(args) => install(args).await,
}
```

Rust enums are **algebraic data types** — each variant can carry different data.
`match` is exhaustive: the compiler requires you to handle every variant.

---

## Closures

```rust
// No arguments, captures env
let greeting = || format!("Hello, {name}!");

// One argument
let doubled = |x: i32| x * 2;

// Longer body with braces
let result = some_list
    .iter()
    .filter(|item| item.is_valid())
    .collect();
```

Closure traits:
- `FnOnce`: called once, may consume captures.
- `FnMut`: called multiple times, may mutate captures.
- `Fn`: called multiple times, shared access to captures.

---

## Iterators

The iterator trio: `.iter()` (borrow), `.iter_mut()` (mutable borrow),
`.into_iter()` (consume/own).

Common adapters:
```rust
vec.iter()
    .filter(|x| x > 0)       // keep matching elements
    .map(|x| x * 2)           // transform each element
    .take(10)                  // first 10
    .enumerate()               // add index: (i, x)
    .collect::<Vec<_>>()       // gather into a collection
```

`collect::<Result<Vec<_>>>()` short-circuits on the first error.

---

## Smart pointers: `Box`, `Rc`, `Arc`

| Type | Heap? | Thread-safe? | Use case |
|------|-------|-------------|---|
| `Box<T>` | Yes | — | Single owner on heap |
| `Rc<T>` | Yes | No | Multiple owners, single thread |
| `Arc<T>` | Yes | Yes | Multiple owners, multiple threads |

Clone an `Arc` to get a new pointer to the same data.  The data is dropped when
the last `Arc` is dropped.

---

## Traits

A trait is an interface — a set of methods a type must implement:

```rust
pub trait Summary {
    fn summarize(&self) -> String;
}

impl Summary for Article {
    fn summarize(&self) -> String {
        format!("{} by {}", self.title, self.author)
    }
}
```

Trait bounds in generics:
```rust
fn print_summary<T: Summary>(item: &T) {
    println!("{}", item.summarize());
}
// or with `impl Trait` syntax:
fn print_summary(item: &impl Summary) { ... }
```

---

## Async/await and Tokio

```rust
async fn fetch(url: &str) -> Result<String> {
    let response = reqwest::get(url).await?;
    let text = response.text().await?;
    Ok(text)
}
```

- `async fn` returns a `Future` — it doesn't run until you `await` it.
- `await` suspends the current task until the future completes.
- Tokio is the runtime that drives futures and manages threads.

For blocking (non-async) work in an async context:
```rust
tokio::task::spawn_blocking(|| {
    // safe to block here
    expensive_sync_operation()
}).await?
```

---

## Error handling: thiserror and miette

**thiserror** — for library errors (typed, composable):
```rust
#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),
    #[error("parse error: {0}")]
    Parse(#[from] serde_json::Error),
}
```

**miette** — for application errors (pretty display for end users):
```rust
// Convert any error to a miette::Report
some_result.into_diagnostic()?

// Add context to an error
some_result.into_diagnostic().context("doing the thing")?

// Bail with a message
miette::bail!("could not find {}", path.display());
```

---

## RAII and `Drop`

Resources (files, network connections, temp dirs) are tied to values.  When a
value goes out of scope, its `Drop` implementation runs automatically.

```rust
{
    let tmp = tempfile::tempdir()?;  // directory created
    // do work
}  // tmp dropped → directory deleted, even on early return/panic
```

No manual cleanup needed.  No leaks on error paths.

---

## The builder pattern

```rust
let client = Installer::new()
    .with_download_client(client)
    .with_target_platform(platform)
    .with_execute_link_scripts(true)
    .install(&prefix, packages)
    .await?;
```

Each `with_*` method returns `Self`, enabling a fluent chain.  The final method
consumes the builder and produces the result.  This avoids long argument lists
and makes configuration readable.

---

## `include_str!` and `include_bytes!`

```rust
const TEMPLATE: &str = include_str!("../templates/default.lua");
const ICON: &[u8]    = include_bytes!("../assets/icon.png");
```

Files are read at compile time and embedded in the binary.  The path is relative
to the source file that contains the macro.

---

## String types

| Type | Description | When to use |
|---|---|---|
| `String` | Owned, heap-allocated UTF-8 | When you need to own or modify a string |
| `&str` | Borrowed string slice | Function parameters, string literals |
| `&'static str` | Borrowed from binary | Constants, literal strings |
| `Cow<'static, str>` | Either borrowed or owned | Generic functions that accept both |
| `PathBuf` | Owned file path | When you need to own or modify a path |
| `&Path` | Borrowed path slice | Function parameters for paths |
| `OsStr`/`OsString` | Non-UTF-8 paths | When working with raw OS paths |

---

## `format!` and string interpolation

```rust
let s = format!("hello {name}");           // variable by name
let s = format!("hello {}", name);         // positional
let s = format!("{:.2}", 3.14159);         // 2 decimal places
let s = format!("{:?}", my_struct);        // debug format
let s = format!("{:#?}", my_struct);       // pretty debug format
let s = format!("{:>10}", "right");        // right-align in 10 chars
```
