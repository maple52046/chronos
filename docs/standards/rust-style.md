# Rust Style Standard

Rust is the primary implementation language of this project. This standard
distills the **Rust API Guidelines**, the **Linux-kernel Rust coding
guidelines** (maintained by the Rust-for-Linux community with Google and
Microsoft), and the Rust-code subset of Google's **Comprehensive Rust** style
guide. It keeps the rules that drive day-to-day judgment and is self-contained
for open-source use.

> Sources: <https://rust-lang.github.io/api-guidelines/>,
> the kernel `Documentation/rust/coding-guidelines.rst`, and
> <https://github.com/google/comprehensive-rust/blob/main/STYLE.md>. Formatting
> is enforced by `rustfmt`; lints by `clippy`. This document covers the judgment
> calls those tools cannot make.

## Baseline

- **Stable Rust, latest edition** (2021 or newer). Pin the toolchain with a
  `rust-toolchain.toml` so contributors and CI agree.
- **Format with `rustfmt` default settings.** The idiomatic Rust style follows
  from the defaults (e.g. 4 spaces for indentation, not tabs). Never hand-format
  around it. Note that `rustfmt` does **not** check comments or documentation —
  those conventions are maintained manually.
- **Lint with `clippy`**; treat warnings as errors in CI
  (`cargo clippy --all-targets --all-features -- -D warnings`).
- Run `cargo fmt --check`, `cargo clippy`, and `cargo test` before committing.
- A `#[allow(...)]` must be local and justified by a comment.

## Modules & imports

- Group `use` statements: `std`, then external crates, then `crate`/`self`/
  `super`. `rustfmt` orders within a group; keep the groups separated.
- Prefer importing the item you call; import the parent module when it improves
  readability or avoids name clashes (e.g. `use std::fmt;` then `fmt::Display`).
- Avoid glob imports (`use foo::*`) except for a crate's documented `prelude` or
  inside a test module (`use super::*;`).
- Expose a deliberate public surface with `pub`; keep everything else private.
  Re-export the intended API from the crate root.

## Types & ownership

- Make illegal states unrepresentable: prefer enums and newtypes over bare
  `bool`/`String`/`i64` flags. A `Newtype(u64)` beats a naked `u64` for an id.
- Borrow in function signatures (`&str`, `&[T]`) rather than taking owned values
  unless ownership is genuinely needed.
- Accept generic bounds (`impl AsRef<Path>`, `impl IntoIterator`); return
  concrete types. Use `impl Trait` in return position rather than boxing when a
  single type suffices.
- Derive the obvious traits (`Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`,
  `Default`) when correct and cheap; do not derive `Clone` to dodge a
  borrow-checker error.
- Implement `From`/`TryFrom` for conversions; prefer `TryFrom` when conversion
  can fail.

## Error handling

- Functions that can fail return `Result<T, E>`; do not signal errors via
  sentinel values or panics.
- **Libraries** define concrete, enumerated error types (e.g. with `thiserror`)
  so callers can match. **Binaries / top-level glue** may use `anyhow` for
  ergonomic propagation with context (`.context("...")`).
- Never `unwrap()`/`expect()` on fallible runtime input in library or production
  paths. `expect` is acceptable only for invariants that are genuinely
  unreachable, and its message must state the invariant.
- Use `?` for propagation. **Panicking should be very rare** and used only with
  a good reason; in almost all cases a fallible approach (returning a `Result`)
  should be used instead.

## Naming

Follow the [Rust API Guidelines naming conventions](https://rust-lang.github.io/api-guidelines/naming.html):
`snake_case` for modules, functions, and variables; `UpperCamelCase` for types,
traits, and enum variants; `SCREAMING_SNAKE_CASE` for constants and statics.
Getters are named for the field (`fn name()`, not `fn get_name()`). Conversions
follow the `as_`/`to_`/`into_` cost convention.

Do **not** repeat the namespacing introduced by modules and types in item names.
For example, prefer `gpio::LineDirection::In` over
`gpio::gpio_line_direction::GPIO_LINE_DIRECTION_IN`. When wrapping an existing
external concept (e.g. a C API), keep the name as close as reasonably possible
to the original, adjusting only the casing to Rust conventions.

## Documentation (rustdoc)

- Document every public item with `///`; the crate root with `//!`. A reader
  should be able to use the item without reading its body.
- The **first paragraph must be a single sentence** briefly describing what the
  item does; further explanation goes in later paragraphs.
- Unsafe functions must document their safety preconditions under a `# Safety`
  section. Functions that may panic must describe when under a `# Panics`
  section. Usage examples go under `# Examples` (they are compiled and run by
  `cargo test`, so keep them correct).
- Link Rust items (functions, types, constants) appropriately; `rustdoc` creates
  the link automatically from `` [`Item`] ``.

## Comments

Comments explain **intent**, not mechanics, and are for *implementation
details*, not API users (that is what `///` documentation is for). Never narrate
what the code does. The full content rule is binding: see
[`comment-content-rule.md`](comment-content-rule.md).

Conventions (not checked by `rustfmt`):

- Write `//` comments in Markdown, the same as doc comments, so content can move
  between the two kinds easily.
- Capitalize the first letter of a sentence and end with a period — including
  tagged comments such as `// SAFETY:`, `// TODO:`, and `// FIXME:`.
- **Every `unsafe` block must be preceded by a `// SAFETY:` comment** explaining
  why the code inside is sound (cannot trigger undefined behavior). Even when the
  reason looks trivial, the comment confirms there are no extra implicit
  constraints. This is distinct from a `# Safety` doc section, which states the
  *contract* callers/implementors must uphold; `// SAFETY:` shows why a specific
  call/impl *respects* that contract.

`TODO` format: `// TODO: <link/ref> - <explanation>.`

## Functions, state & structure

- Keep functions small and focused; reconsider any function over ~40 lines.
- Prefer iterators and combinators over manual index loops where they read more
  clearly; do not contort code to avoid a `for`.
- Avoid mutable global state (`static mut` is forbidden); thread state through
  arguments or the injected ports from the architecture standard.
- Keep `fn main()` a thin composition root that wires drivers/adapters into use
  cases.

## Examples in docs & teaching code

- Code samples should be short and do something meaningful. Avoid generic
  placeholders like `Foo`/`Bar`/`Baz`; use descriptive names from the project's
  domain.
- When showing inline code, use `rustfmt` spacing (`3 * x`, not `3*x`).

## Concurrency & async

- Choose one async runtime at the composition root (e.g. `tokio`); the domain
  and use-case layers stay runtime-agnostic behind ports.
- Prefer message passing and ownership transfer over shared mutable state; when
  sharing is required, use `Arc<Mutex<_>>`/`Arc<RwLock<_>>` deliberately and
  keep critical sections small. Never hold a lock across an `.await`.

## Testing

- Unit tests live in a `#[cfg(test)] mod tests` block beside the code;
  integration tests live in `tests/`. Use `assert!`/`assert_eq!` freely in tests
  (the production-code ban on panics for control flow does not apply here).
- Test behaviour through the ports with fakes, not the concrete drivers, so the
  suite runs without a real clock, filesystem, or network.

## Prose

- Write documentation and comments in American English ("initialize", not
  "initialise"); prefer terminology from the official Rust Book.
- Be precise: avoid weasel words ("likely", "probably", "usually") when a
  definite statement can be made; if a behaviour is conditional, state the
  condition.

## Parting rule

**Be consistent** with surrounding code; let consistency converge toward this
standard over time rather than freezing an older local style.
