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
3. Mutate the code to verify tests catch real bugs
4. Refactor to improve the design

## Style Guidelines

1. Use [Microsoft's checklist of Rust guidelines](https://microsoft.github.io/rust-guidelines/guidelines/checklist/index.html).
2. Favour the use of hexagonal-style architecture to enable testing
3. Use comments to explain the intent behind the code. You don't need to write comments to explain what the code is doing.
4. Always include *doc* comments for public functions, structs, enums, and methods. Always document the parameters, returns and errors.

## System Design

Lean towards hexagonal architecture: core domain components shouldn't depend on non-domain code.

## Before Committing

1. Ensure all tests pass `cargo test`
2. No compile warnings `cargo build`
3. Code is formatted `cargo fmt --check`
4. Clippy has no warnings `cargo clippy -- -D warnings`