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

**When implementing a new command/slice, prefer the `command-slice` skill**
(`.agents/skills/command-slice/`). It orchestrates assess → capture → design →
build → compare with model-pinned subagents, drives the live FS-UAE reference and
the NextExpress server for you, and enforces the project's hardening rules
(`resources/hardening.md` is the §10 checklist). The steps below are what that skill
automates — follow them directly only for one-off changes outside the skill.

Do not add more code than is necessary for a particular feature/problem. Always start with a failing test. Use mutant testing to help improve the code.

Code is written test-first with test driven development:

1. Write a failing test
2. Write the minimum code to pass the test
3. Run `cargo nextest run` to execute the test suite
4. Run `make mutants-diff` to verify the tests for your changes catch real bugs
5. Refactor to improve the design

## Mutation Testing

Use `cargo-mutants` to check for insufficient testing on every implementation
turn. It is configured in `rust/.cargo/mutants.toml` to run tests through
`cargo-nextest`. The routine gate is **diff-scoped** — mutate only the
lines you changed:

```sh
make mutants-diff                 # working-tree changes vs the last commit
make mutants-diff DIFF_BASE=main  # everything on a branch
```

The target generates the diff with crate-relative paths so cargo-mutants
actually filters to your changes — a repo-root diff reports "No mutants to
filter". You can still scope to a single file with
`cargo mutants --file path/to/file.rs` from `rust/`.

The **full sweep** (`make mutants`) covers every mutant in the crate
(1,800+ as of July 2026, ~6–9 hours serial) and is a scheduled/background
job, never a per-commit gate — shard it across parallel runs with
`make mutants MUTANTS_ARGS='--shard k/n'`. Do not shrink it with
`exclude_globs` on wire-const modules: the smoke-killed mutants there are
exactly the coverage this project cares about.

Treat surviving mutants as test gaps: add or strengthen tests before
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
4. Check for insufficient tests on your changes with `make mutants-diff`
   (the full `make mutants` sweep is a scheduled job, not a commit gate)
5. Update the SYSTEM.md document to reflect current design. Ensure the diagram reflects the current system.
6. **User-facing interaction is verified live and agent-driven by the
   `command-slice` skill — not by a human typing.** Scripted byte-equality tests
   have real blind spots (per-keystroke echo, line terminators, on-screen
   rendering; see the 2026-06-11 root-cause analysis and
   `designs/2026-06-12-utf8-hotkeys-flagmark-design.md` §6.3). The skill closes
   this in Stage 5: two independent agents drive the **live NextExpress server**
   and the **live FS-UAE board** character-at-a-time and cross-mark each other's
   observations (the parity guarantee). Residual limitation, accepted knowingly:
   agents reading telnet tokens cannot observe true local per-keystroke echo — so
   for **interactive / pager / hotkey** slices the skill prompts the operator for
   an optional hands-on terminal glance (`resources/hardening.md` §10.7). No human
   step is otherwise required.
7. **Every new user-facing surface is grounded in the live FS-UAE reference, not
   just an `express.e` source reading** — the `command-slice` skill's Stage 2 does
   this with agents *while building* the slice. It boots the genuine AmiExpress
   board in the `docker/amiexpress-fsuae` harness (its own per-run container +
   port — see `resources/board-lifecycle.md`), drives it over telnet to capture
   the real wire bytes, saves the transcript under `comparison/transcripts/`, and
   pins the new NextExpress wire to that capture (literals restated in the smoke
   test to guard against drift). Source reading alone is not enough: it omits
   emergent behaviour (which prompt is shown, exact spacing/echo, run-time data)
   and a captured plan can be wrong — e.g. slice D4 (`Z`) was planned as "search
   the current area" but the live reference showed `Z` *always* opens the internal
   `getDirSpan` directory prompt. **Door-shadow caveat:** the stock deployment
   installs `AquaScan` icons over `CS`/`F`/`FR`/`N`/`NSU`/`SCAN`/`SENT`, so those
   tokens capture the door while every other token (`Z` included) captures the
   genuine internal command; when the door capture and `express.e` conflict, the
   skill **halts and asks** (express.e-wins default — `resources/hardening.md`
   §10.3). The wire is grounded before it is pinned, and every reference session
   ends with a clean `G Y` logoff (the FS-UAE node-spin hazard).

Formatting (`cargo fmt`) and clippy (`cargo clippy -- -D warnings`) run
automatically via Claude Code hooks defined in `.claude/settings.json` — fmt
runs after every Rust edit, clippy runs at session stop and blocks if there are
warnings.
