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

## Code Navigation

A `rust-analyzer` LSP is configured for this project (the
`rust-analyzer-lsp` plugin). **Prefer the LSP for symbol-level navigation**
— finding a definition, all references, implementations, or call hierarchy of
a function/struct/enum/method. It understands scope, shadowing, and
re-exports, so it beats grep on names that collide (e.g. `select`, `Runtime`).
The `LSP` tool is *deferred*: load its schema once per session with
`ToolSearch` (`select:LSP`) before the first call, then use
`workspaceSymbol`/`findReferences`/`goToDefinition`/`incomingCalls` etc.
rust-analyzer indexes lazily, so the first query may return nothing — retry
after a moment.

Use **grep / Glob** for non-symbol text: wire strings, comments, config keys,
capture transcripts, and discovering files by name.

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

For narrow changes or when a full mutation run is too expensive, run
`make mutants-diff` to mutate only the lines changed since the last commit
(use `make mutants-diff DIFF_BASE=main` to cover a whole branch). The target
generates the diff with crate-relative paths so cargo-mutants actually filters
to your changes — a repo-root diff reports "No mutants to filter". You can
still scope to a single file with `cargo mutants --file path/to/file.rs` from
`rust/`. Treat surviving mutants as test gaps: add or strengthen tests before
completing the turn, or explicitly report why a surviving mutant is equivalent
or intentionally deferred.

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

## Wire encoding

The NextExpress wire is **valid UTF-8, always**. The legacy board emits
ISO-8859-1 (Amiga) bytes; when porting captured output, re-encode each
high-bit byte to the same code point in UTF-8 (`\xa9` → `\u{a9}`) and
record the departure as a COMMAND_PARITY.md row. Never emit raw bytes
≥ 0x80 outside a valid UTF-8 sequence — the e2e UTF-8 gate
(`tierd_file_list_smoke.rs::utf8_gate_every_session_byte_decodes`)
enforces this. Rust consts carrying re-encoded glyphs are `&str`.

## System Design

* Lean towards hexagonal architecture: core domain components shouldn't depend on non-domain code.
* Prefer simplicity; don't create abstractions before necessary.

## Before Committing

1. Ensure all tests pass `cargo nextest run`
2. No compile warnings `cargo build`
3. Run doctests with `cargo test --doc`
4. Check for insufficient tests with `cargo mutants`
5. Update the SYSTEM.md document to reflect current design. Ensure the diagram reflects the current system.
6. For any slice that changes user-facing interaction: boot the server
   (`cargo run -- nextexpress.toml`, or the built binary with the config path
   as its first argument), connect with a plain UTF-8 terminal client
   (`telnet 127.0.0.1 2323`), and exercise the new surface **by typing** —
   checking per-keystroke echo, line terminators, and on-screen rendering.
   Scripted byte-equality tests cannot observe these (see the 2026-06-11
   root-cause analysis; `designs/2026-06-12-utf8-hotkeys-flagmark-design.md`
   §6.3). Capture replay is faithful to the capture's blind spots, so a human
   has to look at the real terminal once per user-facing slice.
7. **Verify every new piece of user-facing experience against the live
   FS-UAE reference — not just against `express.e` source reading.** For
   any slice that adds or changes a user-visible surface, boot the
   genuine AmiExpress board in the `docker/amiexpress-fsuae` harness,
   drive it over telnet to *capture the real wire bytes* of the
   behaviour, save the transcript under `comparison/transcripts/`, and
   pin the new NextExpress wire to that capture (restate the literals in
   the smoke test so they guard against drift). Reading the E source
   alone is not enough: it omits emergent behaviour (which prompt is
   shown, exact spacing/echo, run-time data) and the captured plan can be
   wrong — e.g. slice D4 (`Z`) was planned as "search the current area",
   but the live reference showed `Z` *always* opens the internal
   `getDirSpan` directory prompt. Mind the door-shadow caveat: the stock
   deployment installs `AquaScan` icons over `CS`/`F`/`FR`/`N`/`NSU`/
   `SCAN`/`SENT`, so those tokens capture the door, while every other
   token (`Z` included) captures the genuine internal command (see the
   harness notes in the auto-memory and `COMMAND_PARITY.md`). Do this
   *while building* the slice, so the wire is grounded before it is
   pinned, and end every reference session with a clean `G Y` logoff
   (the FS-UAE node-spin hazard).

Formatting (`cargo fmt`) and clippy (`cargo clippy -- -D warnings`) run
automatically via Claude Code hooks defined in `.claude/settings.json` — fmt
runs after every Rust edit, clippy runs at session stop and blocks if there are
warnings.
