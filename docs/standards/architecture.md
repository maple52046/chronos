# Architecture Standard

This project follows **The Clean Architecture** (Robert C. Martin). This
document adapts its single load-bearing rule — *The Dependency Rule* — to a
Rust codebase. It is self-contained so the project can be used and open-sourced
independently.

> Source of the underlying principles: Robert C. Martin, *The Clean
> Architecture*, 2012. <https://blog.cleancoder.com/uncle-bob/2012/08/13/the-clean-architecture.html>

## The Dependency Rule

Source-code dependencies point **inwards only**. An inner layer must never name
anything declared in an outer layer (no function, type, constant, or data
format defined further out). Data crossing a boundary is a plain structure
owned by the inner layer — never a framework-shaped or driver-shaped value.

In Rust terms: an inner module/crate must not `use` an outer module/crate, and
must not depend on it in `Cargo.toml`. Compilation direction is the mechanical
proof of the rule — if the domain crate compiles without the adapter/driver
crates, the rule holds.

## Layers (inner to outer)

1. **Domain (innermost).** The core types of the system and the trait contracts
   that describe its behaviour. Pure data and abstractions: no I/O, no async
   runtime, no clock, no filesystem, no third-party framework beyond a minimal
   data-modelling/serialization dependency. Domain errors are concrete types
   (e.g. via `thiserror`), never `anyhow::Error`.
2. **Use cases.** Application policy: orchestrate the domain types to fulfil a
   request. Depends only on the domain layer and on the **abstract ports**
   (traits) below — never on a concrete adapter.
3. **Interface adapters.** Convert between the domain form and the outside
   world: parsers/serializers, persistence backends, the CLI/HTTP surface.
   They depend inwards on the domain/use-case layers and implement the ports.
4. **Frameworks & drivers (outermost).** Concrete details: the async runtime
   (e.g. `tokio`), the system clock, the filesystem, databases, network
   clients, third-party libraries. Mostly glue wired together in `main`.

## Ports (cross dependencies via trait inversion)

When an inner layer must trigger work that lives further out (persist data, read
a clock, call a service), it depends on an **abstract port** — a trait defined
inward — and the outer layer provides the implementation. The use-case layer
references only these traits, so adding a new backend, a new transport, or a new
driver never edits the core.

Guidelines for ports in Rust:

- Define the trait in the inner layer; implement it in the outer layer.
- Keep traits small and intention-revealing (one responsibility per port).
- Inject implementations via generics (`fn run<C: Clock>(clock: &C)`) or trait
  objects (`Arc<dyn Clock>`); choose per call-site, not globally.
- Prefer constructor injection over global singletons or `static` state.

## Project rules derived from the above

- The domain MUST NOT depend on adapters, drivers, frameworks, an async runtime,
  a clock, or any I/O facility.
- A concrete adapter depends on the core's traits; the core never names a
  specific adapter. Adapters are selected at the composition root.
- Drivers sit behind ports. Swapping one driver for another is a new
  implementation of an existing trait, not a change to the domain or use cases.
- Data crossing boundaries is a domain-owned type, never a driver-specific
  struct, a raw row, or a serde value leaking outward.
- Side-effecting details (which database, which transport, which time source)
  are *details* and live at the edges.

## Recommended layout

A single binary crate may start with modules and graduate to a Cargo workspace
as boundaries harden. Either way, the dependency arrows must point inward:

```
src/
  domain/        # types + trait contracts (no I/O, no runtime)
  usecases/      # orchestration; depends on domain + ports only
  adapters/      # parsers, persistence, CLI/HTTP; implement ports
  drivers/       # runtime, clock, db, network glue
  main.rs        # composition root: wire adapters/drivers into use cases
```

As a workspace, the same layers become crates whose `Cargo.toml` dependency
edges enforce the rule at compile time.

## Testability consequence

Because policy does not depend on details, use cases and domain logic are
unit-testable without a runtime, without a real clock, and without touching the
filesystem or network — fakes/mocks implement the ports. This is the property
the architecture exists to guarantee.
