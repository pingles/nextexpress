# Tier A — Quick wins

Small, common menu commands that don't depend on a new subsystem.
Each slice in this file ships one command end-to-end and is sized to
fit a 15–20 minute TDD session.

See [SLICES.md](../SLICES.md) for the schema-growth principle, asset
inventory and adapter-contract checklist.

## Common shape

Every slice in this tier:

- adds an enum variant on `MenuCommand` (`rust/src/app/menu_command.rs`)
  plus a matching parse-test;
- wires a dispatch arm in `MenuFlow::run`
  (`rust/src/app/menu_flow/mod.rs`);
- emits the legacy wire text verbatim, with the `// amiexpress/express.e:NNNN`
  comment carrying the original line beside the byte literal;
- carries a smoke test in the file's wire-and-smoke closing slice.

The toggles in this tier (`X`, `M`, `Q`) introduce the boolean
presentation flags backfilled onto `Session` per the schema-growth
principle — they were deferred from `AcceptConnection`'s ensures
clause in Slice 7 and land here with their first reader.

## Slice A1 — `T` (current date and time) — **Done**

- **In Scope**
  - Parser: `MenuCommand::ShowTime`, recognised case-insensitively as
    a no-arg `T`.
  - Wire text: `It is <MM-DD-YY> <HH:MM:SS>` formatted exactly as
    `internalCommandT()` does (`amiexpress/express.e:25622-25644`,
    `FORMAT_USA` — note: legacy `FORMAT_USA` produces a two-digit
    year, *not* `YYYY` as the original draft of this slice said).
  - Rendering uses `time::macros::format_description!` for the
    `MM-DD-YY HH:MM:SS` portion, per the AGENTS.md "favour idiomatic
    Rust" rule. Time is in UTC; local-offset support is a future
    refinement.
- **Out of Scope**
  - Time-remaining display (that's `T` *plus* "time used today"
    accounting — covered by Tier A's `S` slice and Tier I's
    `daily_byte_cap`).
- **Why it lands first**: zero new domain — pure presentation.

## Slice A2 — `VER` (version banner) — **Done**

- **In Scope**
  - Parser: `MenuCommand::ShowVersion`.
  - Wire text adapted from `internalCommandVER()`
    (`amiexpress/express.e:25688-25698`): the legacy `AmiExpress 5
    Copyright ©2018-2023 Darren Coles` header, the `Original
    Version:` label, and the two author lines (Thomas, Hodge) —
    each preserved verbatim. Trailed by `NextExpress
    <CARGO_PKG_VERSION> (<git-sha>) Copyright ©2026` so the
    operator can pin a running session to a specific build.
  - Implementation: `VERSION_BANNER` const built with `concat!` +
    `env!("CARGO_PKG_VERSION")` + `env!("NEXTEXPRESS_GIT_SHA")`.
- **Out of Scope**
  - Registration key display — the legacy emits the registered
    licensee; NextExpress is unregistered and elides the line.

## Slice A3 — `S` (user stats screen)

- **In Scope**
  - Parser: `MenuCommand::ShowStats`.
  - Reads the existing accessors on `User`
    (`slot_number`, `access_level`, `times_called`,
    `times_called_today`, `last_call`, `messages_posted`,
    `time_used_today`) — all already present in
    `rust/src/domain/user/mod.rs`. Note: there are **no**
    `bytes_uploaded_total` / `bytes_downloaded_total` fields on
    `User` yet (transfer accounting is Tier I); the byte lines of
    the legacy report are deferred to slice A11.
  - Wire text mirrors `internalCommandS()`
    (`amiexpress/express.e:25540-25608`) line by line, with the same
    `[32mLabel[33m:[0m value` ANSI prefixes.
- **Out of Scope**
  - The full multi-page report (`secStatus`, `secBulletin`,
    `onlineBaud`); start with the seven lines every user expects and
    grow as later slices add the fields.

## Slice A4 — menu-prompt parity (incl. mins-left)

**Recast.** The original framing — "extend the `T` command with a
`Time remaining: <m>` line" — had no parity basis: `internalCommandT`
(`amiexpress/express.e:25622`) is purely the date/time clock, and the
legacy surfaces time-remaining in the **menu prompt**, not on `T`. The
only `Time remaining` string in the legacy is the FTP response at
`:28934`. So this slice instead brings the menu prompt up to legacy
parity.

- **In Scope**
  - Replaces the simplified `Command: ` prompt with the legacy
    `displayMenuPrompt` default format
    (`amiexpress/express.e:28413-28421`):
    `<bbsName> [<confNum>:<confName>] Menu (<mins> mins. left): `
    with the legacy ANSI colour run. The multi-msgbase conference
    label is `"<name> - <msgbase>"` (`:28416`).
  - `<mins>` is `time_remaining.as_secs() / 60`, mirroring the legacy
    `Div((timeTotal - timeUsed), 60)`. Read via
    `MenuSession::time_remaining()` (delegates to `Session::time_remaining`,
    Slice 14).
  - Introduces `Config.bbs_name` (legacy `cmds.bbsName`; default
    `NextExpress`) and `AppServices::bbs_name()`.
  - Rendering split: `wire_text::render_menu_prompt` (pure bytes) and
    `session_presenter::format_menu_prompt` (resolves the conference
    label + converts the budget to whole minutes).
- **Out of Scope**
  - The sysop-supplied custom-prompt (MCI) branch
    (`amiexpress/express.e:28409-28412`).
  - Seeding a non-zero per-call time budget for the default sysop —
    the seed currently has zero limits, so the prompt reads
    `(0 mins. left)` until a budget is configured; that's a seed/config
    concern, not a prompt-rendering one.

## Slice A5 — `H` (BBS help screen) — **Done**

- **In Scope**
  - Parser: `MenuCommand::ShowHelp`.
  - Adapter: `ScreenRepository::bbs_help_screen()` loads
    `<bbs-loc>/BBSHelp.txt` if present; the dispatch arm falls back
    to the verbatim legacy message `Sorry Help is unavailable at
    this time.` (`amiexpress/express.e:25083`) when the adapter
    returns empty bytes (matching the existing `logoff_screen`
    "absent = empty" contract).
  - Per-security-level walk (`BBSHelp5.txt`, `BBSHelp10.txt`, …) is
    *not* implemented; a future refinement can mirror the
    `default_menu` walk if a slice needs it.
- **Out of Scope**
  - The `^` topic-help lookup (slice A10 below).

## Slice A6 — `X` (expert mode toggle)

- **In Scope**
  - Adds `User.expert_mode: bool` (first read here).
  - Toggles the field, writes back via `UserRepository`, emits
    `Expert mode enabled` / `Expert mode disabled` per
    `amiexpress/express.e:26115-26120`.
  - Menu-prompt rendering branches on `expert_mode`: when set, the
    full `Menu.txt` is *not* re-displayed before each prompt (matches
    legacy `displayMenuPrompt`).
- **Out of Scope**
  - Per-conference menu expert variants (legacy supports them; defer).

## Slice A7 — `?` (display menu)

- **In Scope**
  - Parser: `MenuCommand::ShowMenu`.
  - When `User.expert_mode == true`, prints `Conf<N>/Menu.txt`
    (`amiexpress/express.e:24594-24598`).
  - When `expert_mode == false`, no-op (the menu has just been
    displayed by the loop).
- **Depends on**: A6 (the `expert_mode` field).

## Slice A8 — `M` (ANSI mode toggle) and the existing-`M` cleanup

- **In Scope**
  - Toggles the colour preference. **Reconcile first:** an
    `ansi_colour` flag already lives on `User`
    (`rust/src/domain/user/profile.rs`, persisted in
    `user/persisted.rs` and the SQLite schema) — there is **no**
    `Session.ansi_colour`. The toggle therefore either flips the
    existing `User.ansi_colour` and writes back via `UserRepository`
    (same shape as A6's `expert_mode`), or introduces a deliberate
    session-level override that shadows the persisted preference.
    Pick one before implementing and note the choice here.
  - Re-binds the parser: `MenuCommand::Scan(ScanArg::All)` (the
    current `M` binding) moves to `MS`; `MenuCommand::AnsiToggle` is
    the new `M`.
  - Wire text `Ansi Color On` / `Ansi Color Off`
    (`amiexpress/express.e:25241-25247`).
  - Outgoing writes that contain `\x1b[…m` escapes are stripped at
    the terminal adapter when colour is off. **This stripping does
    not exist today** — `file_screen_repository`'s
    `normalise_to_crlf` currently *preserves* escapes — so this
    slice introduces the new adapter surface, it is not a deferred
    hook waiting to be read.
- **Out of Scope**
  - Per-screen ANSI substitution (RIP mode is its own field, see
    `cmds-misc.md`).
- **Pairs with**: Tier B's `cmds-mail-finish.md`, which owns the
  `MS` half of the scan-mail reshape. Landing `M` → `AnsiToggle`
  here without `MS` there leaves scan-all unreachable, so the two
  must land together (or `MS` first).

## Slice A9 — `Q` (quiet mode toggle) — **Done**

- **In Scope**
  - Adds `Session.quiet_mode: bool` (first read here).
  - Toggle emits `Quiet Mode On` / `Quiet Mode Off`
    (`amiexpress/express.e:25508-25512`).
  - Per the existing OLM stub in
    [cmds-comm.md](cmds-comm.md), quiet sessions don't receive
    inter-node messages once OLM lands.

## Slice A10 — `^` (context help screen)

- **In Scope**
  - Parser: `MenuCommand::TopicHelp(String)` — accepts
    `^<topic>` or `^ <topic>`.
  - Adapter looks up `<bbs-loc>/help/<topic>.txt`; if not found, falls
    back one character at a time per
    `amiexpress/express.e:25094-25109` (`internalCommandUpHat`'s
    truncation-and-retry loop).
- **Out of Scope**
  - SCREEN-style colour codes inside help files beyond what
    `displayFile` already handles.

## Slice A11 — `S` extended report (full legacy stats screen)

- **In Scope**
  - Extends slice A3 to the full legacy `internalCommandS()` output
    (`amiexpress/express.e:25540-25608`): `secStatus`, `secBulletin`,
    `onlineBaud`, `messagesPosted`, `timesCalled`, plus the
    Tier-I-only fields (`bytes_uploaded_total`,
    `bytes_downloaded_total`) once they exist.
- **Out of Scope**
  - Per-conference breakdown — that's a separate Tier I refinement.
- **Why split from A3**: A3 lands a usable baseline in one TDD turn.
  This slice ships once the remaining fields exist on `User` and
  `ConferenceMembership` (Tier I).

## Slice A12 — `NS` non-stop pause / break-out (cross-cutting)

- **In Scope**
  - Adds a session-scoped `non_stop` flag plumbed through the
    Terminal adapter.
  - Any command that paginates (`F`, `FR`, `Z`, `B`, `MS`, `R` sub-prompt
    `L`ist) honours the legacy `paramsContains('NS')` token
    (`amiexpress/express.e:24627, 24644, 26170, …`) and the
    `<SPACE> = pause` / `<CR> = continue` / `Q` / `NS` runtime
    keystrokes during a paginated stream.
  - Deferred from slice D2 once `H`'s `NS` token paths are unified.
- **Out of Scope**
  - Per-conference default pause settings — that's a config concern.
- **Why here**: the flag is read by file, mail and bulletin
  commands across multiple tiers; landing it as a single quickwin
  prevents drift between the readers.

## Slice A-wire — Tier A wire-and-smoke

- **In Scope**
  - Composition-root wiring: every new command above is reachable
    from a `cargo run` binary against the default `nextexpress.toml`.
  - Smoke test (`tests/quickwins_smoke.rs` — this file already
    exists and covers `T`, `VER`, `Q`, `H`; extend it per command
    as `S` / `X` / `?` / `^` / `M` land). Per AGENTS.md guideline 6
    it drives the `TelnetListener` **in-process** (bind on
    `127.0.0.1:0`, `tokio::spawn` the accept loop, connect a tokio
    client) rather than spawning the binary. It logs in as the
    seeded sysop and sends each Tier-A command in turn — asserting
    the verbatim wire-bytes for the response and that the menu
    prompt re-appears after each.
- **Out of Scope**
  - Performance / load shape — smoke proves reachability, not
    throughput.
