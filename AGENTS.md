# AGENTS.md

NextExpress is a port of the AmiExpress BBS system to Rust, enabling it to be run
across more platforms.

We strive for compatibility to the existing behaviour where possible.
However, we choose a more modern approach where sensible (e.g. configuration
via files rather than a separate program).

## Repository

```
amiexpress/  # Amiga E rewrite of AmiExpress. Not to be modified, used as source
specs/       # Allium specs describing behaviour of E rewrite above.
rust/        # Implement your Rust code here.
```
[Allium specs](https://juxt.github.io/allium/) in `specs` describe the key behaviour. Favour using this to determine how something should work before looking at the Amiga source.

Always use the `amiexpress` source when referencing original strings/messages/commands etc.

## Key Workflow

Do not add more code than is necessary for a particular feature/problem. Always start with a failing test. Use mutant testing to help improve the code.

Code is written test-first with test driven development:

1. Write a failing test
2. Write the minimum code to pass the test
3. Run `cargo nextest run` to execute the test suite
4. Run `cargo mutants` to verify tests catch real bugs
5. Refactor to improve the design

## Mutation Testing

Use `cargo-mutants` to check for insufficient testing on every implementation
turn. It is configured in `rust/.cargo/mutants.toml` to run tests through
`cargo-nextest`. Run it from the Rust project directory:

```sh
cd rust
cargo mutants
```

For narrow changes or when a full mutation run is too expensive, still run a
focused cargo-mutants command such as `cargo mutants --in-diff main...HEAD` or
`cargo mutants --file path/to/file.rs`. Treat surviving mutants as test gaps:
add or strengthen tests before completing the turn, or explicitly report why a
surviving mutant is equivalent or intentionally deferred.

## Style Guidelines

1. Use [Microsoft's checklist of Rust guidelines](https://microsoft.github.io/rust-guidelines/guidelines/checklist/index.html).
2. Favour the use of hexagonal-style architecture to enable testing
3. Use comments to explain the intent behind the code. You don't need to write comments to explain what the code is doing.
4. Always include *doc* comments for public functions, structs, enums, and methods. Always document the parameters, returns and errors.
5. **Favour idiomatic Rust over line-for-line translations of the legacy `E`
   source.** The `amiexpress/` tree is the authority on *behaviour* and
   *user-visible strings* — not on how to write Rust. When porting a procedure,
   match the legacy's outputs and side effects, then express the implementation
   with the standard library, the `time` crate's format descriptions, iterator
   combinators, etc., rather than hand-rolled loops mirroring the E code.
   Parity is at the wire / behaviour boundary, not the line boundary.
6. **End-to-end tests run the listener in-process, not as a spawned binary.**
   Bind a `TelnetListener` on `127.0.0.1:0` with a `Runtime` built from
   in-memory adapters, `tokio::spawn` its accept loop, and connect with a
   tokio client. A child-process boot is slower, harder to debug, and only
   buys parity with the binary's argv/config-load path, which is covered
   by its own startup tests. The existing `tests/quickwins_smoke.rs` is the
   reference shape for new command-family smokes.

## System Design

* Lean towards hexagonal architecture: core domain components shouldn't depend on non-domain code.
* Prefer simplicity; don't create abstractions before necessary.

## Before Committing

1. Ensure all tests pass `cargo nextest run`
2. No compile warnings `cargo build`
3. Run doctests with `cargo test --doc`
4. Check for insufficient tests with `cargo mutants`
5. Update the SYSTEM.md document to reflect current design. Ensure the diagram reflects the current system.

Formatting (`cargo fmt`) and clippy (`cargo clippy -- -D warnings`) run
automatically via Claude Code hooks defined in `.claude/settings.json` — fmt
runs after every Rust edit, clippy runs at session stop and blocks if there are
warnings.
