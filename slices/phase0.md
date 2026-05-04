# Phase 0 — Project foundations

Lays down the crate skeleton with hexagonal module boundaries enforced at
compile time. No domain behaviour yet.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 1 — Cargo crate skeleton
- **In Scope**
  - Single `rust/Cargo.toml` crate (`nextexpress`) with `domain`, `adapters` and `app` top-level modules.
  - Module visibility enforces hexagonal direction per `AGENTS.md`: `domain` has no `use` of `adapters` or `app`; `adapters` and `app` may depend on `domain`.
  - A `lib.rs` test (or a clippy lint) catches any inbound `use` from `domain` to the other modules.
  - `cargo test`, `cargo build`, `cargo fmt --check`, `cargo clippy -- -D warnings` all succeed against the empty modules.
- **Out of Scope**
  - Any domain types or behaviour.
  - A `Config` struct, value types like `Bytes`, or `PasswordHashKind` variants. These all land with the slice that first reads them.
  - Splitting into separate crates (revisit only if compile times or external reuse demand it).
