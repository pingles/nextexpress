# NextExpress System Notes

This document captures the current internal design of the Rust implementation
under `rust/` and the larger refactorings worth considering next.

## Suggested refactorings (ordered)

The July 2026 forward-looking review, sequenced in application order.
Item numbers refer to the detailed entries under "Large-scale
refactorings worth considering" and "Forward-looking review additions"
below; the slice-by-slice rationale is under "Suggested order". Effort
and file counts are the review verifiers' adjusted estimates.

| # | Refactoring (item) | Land when | Why it improves the design for what's coming | Files touched | Effort |
|---|---|---|---|---|---|
| 1 | **Unify flag identity to `(conference, name)`** (14) | **Landed** 2026-07-02 | Fixes a live defect: dual `FlaggedKey` identities silently lose flag saves under SQLite and duplicate the `A` listing. D/DS downloads consume this list as the default download set — it must have one identity before anything is pinned against it. | ~10 src/test + 3 docs | 0.5–1 day |
| 2 | **Mutation gate → diff-vs-main** (15) | **Landed** 2026-07-02 | `make check` documents a 6–9 h full sweep nobody runs; agents execute the checklist literally. Aligning the documented and practiced gate keeps TDD+mutants viable as the crate grows through Tier D. | 2 (Makefile, AGENTS.md) | 1–2 h |
| 3 | **Smoke-harness builder** (12 remainder) | **Landed** 2026-07-02 (two-session primitives stage with Tier E) | Six smokes each re-roll ~110–155 lines of harness; FS is the next smoke to be written. Every remaining Tier D slice needs a capture-pinned smoke, so this pays out eight more times. | ~8, test-only | ~1 day |
| 4 | **Clock port in `AppServices`** (16) | **Landed** 2026-07-03 | 48 hardwired `SystemTime::now()` sites mean no test can control the date. N's "-X Days" scan, transfer timestamps, and Tier I daily caps/rollover are all untestable deterministically without it. | ~20 (mechanical one-liners) | ~1 day |
| 5 | **`FileRepository` port prep** (18) | **Landed** 2026-07-03 | Result-ifies the port while the blast radius is 8 call sites, and gives files an identity (`FileAreaRef`). N's date query lands as a since-bounded port method — the contract the D2s SQLite store inherits, instead of client-side filtering. | ~6–9 | 0.5–1 day |
| 6 | **Extract the NextScan scan engine** (17) | First task of the N slice | The pager/dir-walk machine is private to the 862-line `file_list` and welded to F's row source; N is pinned to the same engine. The extraction makes N a thin entry point and serves every later lister (download preflight, FM). | 4–5 | 1–1.5 days |
| 7 | **Prompt-reader merge + `line_for` extraction** (10) | **Landed** 2026-07-03 | Six hand-rolled readers, and the `record_input` idle-stamp is already inconsistent. One reader that stamps internally makes every upcoming prompt (N's date, FM's loops, W, account editor) a one-liner that can't forget the idle clock. | ~6 | 1–1.5 days |
| 8 | **Error-boundary pass** (2 remainder) | **Landed** 2026-07-03 (`UserRepositoryError` folds into row 9) | Port errors diverge four ways; D2s will copy whichever template it finds, and today the prominent one leaks adapter vocabulary into the domain. Pins one opaque-`Backend` convention before the port family doubles. | 12–15 (mostly mechanical) | ~1 day |
| 9 | **Command-style user writes** (1) | After N, before D-T2 | The whole-aggregate upsert silently reverts any concurrent writer and isn't transactional. D-T2's ledger deltas are the first second-writer path; Tier G/H sysop edits and Tier I accounting all depend on delta/patch writes existing. The biggest single item. | ~10–14 incl. tests | 4–6 days |
| 10 | **SQLite schema migrations** (22) | Before D-T2 | No mechanism can alter an existing `users.db`; D-T2's first new column would break every login after upgrade. Versioned migrations make every future schema change (accounting, D2s tables, `row_version`) routine. | 3–5 | 0.5–1 day |
| 11 | **`AuthenticatedCall` struct** (23a) | Before D-T2 | The per-call triple is duplicated across four phase variants with two 8-arm salvage matches; every new per-call field costs ~7 sites. D/U must add transfer tallies — after this, that's a single-site addition. | ~6–7 | 0.5–1 day |
| 12 | **UTF-8 policy re-scope + echo-hole fix** (19) | Before D-T1 | The written "wire is UTF-8, always" contract is violated by the first Zmodem frame; deciding the text-mode/transfer-window scoping up front prevents an ad hoc weakening of the gate mid-slice. Also closes today's Latin-1 echo hole. | ~4 (2 docs + codec + test) | ~0.5 day |
| 13 | **Raw binary channel on `Terminal`** (20) | Opening sub-slice of D-T1 | The stack destroys binary in both directions (lossy decode, dropped `IAC IAC`, no 0xFF doubling, ANSI stripping). `read_bytes`/`write_raw` with BINARY negotiation is the one seam all transfer slices flow through. | ~5–8 | 1–3 days |
| 14 | **Sans-IO Zmodem engine decision** (21) | Shapes D-T1 | Porting `zmodem.e`'s callback-into-serial shape would weld protocol logic to live sockets — untestable, mutants-hostile — and force writing the protocol twice (the smoke harness needs an embedded client; the sans-IO engine plays both roles). | 1 doc now; engine lands inside D-T1..T5 | decision now; 1–2 wks in-slice |
| 15 | **Real `has_access` narrowing** (24) | With the first refusing slice (D/U eligibility or FM/US) | The stub grants every right to any validated account; FM/US are the first commands that must refuse a level-2 user. Minimal per-variant narrowing keeps `has_access` the single choke-point instead of scattering level checks. | ~3 | 0.5–1 day |
| 16 | **`NodePool` → presence registry** (25) | Immediately before WHO | Today nothing can answer "who is online" — no registry, dead `LoggedOn` transitions, pool unreachable from handlers. `NodePresence` + `snapshot_all` is the read-side seam for WHO/WHD, Tier G's node monitor, and the place delivery handles later hang. | ~8–12 | 1–2 days |
| 17 | **`SessionSignal` channel in the terminal** (26) | Opening move of OLM/page | Sessions are unreachable while blocked on a read; the select-in-terminal design (after hoisting the line buffer so delivery can't drop half-typed input) is the delivery lane OLM, page, and later Tier G kick/suspend all reuse — avoiding a session-actor rewrite. | ~5–7 | ~3 days |
| 18 | **Activate the time budget** (27) | During Tier E, before Tier G's G6 | `tick_minute` has zero callers — "mins. left" is frozen and `OutOfTime` unreachable (a parity gap now). Tier G's time +/- needs a live budget to adjust, and Tier I's caps need `time_used_today` to have actually accrued. | ~4–6 + FS-UAE capture | 1–2 days |

Dependencies: 13 depends on 12's policy decision; 14 depends on 13; 17
builds on 16's `NodePresence`; 18 wants 4's clock for a deterministic
smoke; 9 stays schema-free unless 10 lands first. Rows 1–2 are
standalone commits, rows 4–7 cluster around the N slice, rows 9–14 are
the pre-transfer architectural block (~2 weeks), and rows 15–18
deliberately wait for their first consumer, per the "seam lands with
the slice that consumes it" rule.

## Current Shape

The implementation is a hexagonal (ports and adapters) layout split across four
top-level modules under `rust/src/`:

- **`domain/`** — pure behaviour and entities distilled from the Allium specs in
  `specs/`. Aggregates (`Session`, `User`, `Conference`, `ConferenceVisit`,
  `Mail`, `Node`, `File`, `FileArea`) plus the per-session
  `ConferenceActivity` sub-aggregate (owns the `Vec<ConferenceVisit>` +
  `Option<ConferenceScan>` and lives outside the phase enum so it
  survives `Onboarded → Menu`), value objects (`ReadPointers`,
  `MessageBaseRef`, `Bytes`, `ConferenceScan`, and the `pub`
  `FlaggedFiles`/`FlaggedKey` — the session-scoped flagged-file set the
  `F`/`R` pager verbs build, shared by `domain/files`, `domain/session`,
  and `app/menu_flow/file_list`, slice D2f; the `A` alter-flags loop also
  adds (`flag`) and clears (`clear`) it (slices D6a/D6b); the plain-`G`
  logoff confirm reads it via `is_empty()` (slice Ga); slice D5-persist
  saves it on logoff and restores it on logon via the `FlaggedStore` port),
  port traits (`UserRepository`, `ConferenceRepository`, `MailStore`,
  `PasswordHasher`, `CallerLogAppender`, `FileRepository`, `FlaggedStore`), phase-typed session wrappers, the
  `messaging.allium` rule family (`read_mail`, `scan_mail`, `post_mail`,
  `post_comment_to_sysop`, `reply_to_mail`, `forward_mail`, `delete_mail`,
  `edit_mail_header`, `move_mail`, `attach_file_to_mail`), the password
  helpers, caller-log entry shape, and `SessionPolicy`.

- **`adapters/`** — concrete tech: `TelnetListener` (transport),
  `FileConferenceRepository`, `FileScreenRepository` (file-backed assets with
  caching), `FileMailStore` (one JSON file per message),
  `InMemoryMailStores` (registry), `InMemoryUserRepository`,
  `SqliteUserRepository`, `InMemoryFlaggedStore` (process-lifetime flag
  persistence — the default when no `user_storage` path is configured),
  `SqliteFlaggedStore` (durable flag persistence in the same `users.db`,
  a `flagged_files (slot_number, conference, name)` table — selected when
  `user_storage` points to a SQLite path, slice D5-persist),
  `InMemoryFileRepository` (the seeded demo file catalogue, slice D1),
  `InMemoryCallerLog`, `Pbkdf2PasswordHasher`,
  `telnet_line` codec (`read_telnet_line` with an `EchoMode` plus
  `read_telnet_key`, the single-keystroke decoder that lets the NextScan
  pager run true hotkeys and emit its own captured echo bytes — slice D2b).

- **`app/`** — application layer: ports, services, flows, and
  transport-agnostic drivers. Carries application-layer ports
  (`Terminal`, `ScreenRepository`, `MailStores`), configuration types,
  the runtime value (`Runtime` + `AppServices`), the per-connection
  orchestrator (`SessionDriver`), three sub-flows (`LoginFlow`,
  `RegistrationFlow`, `MenuFlow`), a command module under
  `app/menu_flow/*` for each non-trivial menu command (terminal-free
  core fn + terminal-aware handler in the same file; simple
  toggles/queries dispatch inline in `MenuFlow::dispatch`), and the
  `ColourTerminal` decorator
  (`app/colour_terminal`) that strips ANSI SGR escapes from output
  while the `M`-toggled colour mode is off. Production code under `app/` is forbidden from
  importing `crate::adapters`; the boundary is enforced by
  `tests/architecture.rs::app_does_not_depend_on_adapters_in_production_code`.

- **`bootstrap.rs`** — composition root (a single file, no submodules).
  The only module allowed to
  construct concrete adapters: it loads the config, picks the user
  repository (in-memory vs. SQLite), opens one `FileMailStore` per
  known msgbase, builds a `FileScreenRepository`, wires the lot through
  `Runtime::new`, binds the `TelnetListener` and runs the accept loop.
  The `nextexpress` binary's `main` calls `bootstrap::main` and does
  nothing else.

### Architectural invariants

`rust/tests/architecture.rs` walks the source tree and rejects:

1. Any `use` path under `src/domain/` that names `crate::adapters`,
   `crate::app`, or a bare `adapters::` / `app::` sibling.
2. Any non-comment line under `src/domain/` that mentions an
   infrastructure crate or module (`tokio::`, `serde_json::`, `toml::`,
   `std::fs::`, `std::io::`, `std::net::`).
3. Any production-code `use` path under `src/app/` that names
   `crate::adapters` (or the bare sibling). Test modules
   (`#[cfg(test)] mod …`) are excluded since unit tests legitimately
   reach for adapter test doubles; the walker tracks braces to skip
   those blocks. Only `src/bootstrap.rs` is allowed to import adapter
   types.

The infrastructure-reference guard is stronger than a plain import
check — a domain error like `source: serde_json::Error` would fail it
even without an import, so the domain stays free of those infrastructure
types in signatures as well as bodies.

`std::io::` is on the list as of the June 2026 error-boundary pass: the
domain port errors no longer name any concrete I/O type. `MailStoreError`
and `ConferenceRepositoryError` previously embedded `std::io::Error`
directly (`MailStoreError::Io`); they now carry an
infrastructure-agnostic `Backend { source }` variant whose `source` is
the type-erased `Box<dyn Error + Send + Sync>` (`StoreSourceError` /
`ConferenceRepositorySourceError`). Each file adapter owns a
`From<std::io::Error>` impl that boxes its native error at the port
boundary — the single place `std::io::Error` meets the port. `Box<dyn
Error>` is retained deliberately as the opaque source: it is the
standard type-erasing idiom, not an infrastructure-specific leak, so the
guard does not forbid it. (Refactoring 2's larger moves — relocating
`ConferenceRepository` out of the domain and collapsing
`MailStoreError`'s rich variants — remain open.)

### Sync domain, async edges

Every domain port (`UserRepository`, `ConferenceRepository`, `MailStore`,
`PasswordHasher`, `CallerLogAppender`) is **synchronous**. Async only
appears at the application boundary: `Terminal`, `ScreenRepository`, and
`MailStores` are async traits, defined in `app/`. The pattern lets the
messaging rules and session rules stay free of `await`, while the
listener and the menu loop drive I/O cooperatively. The async traits
return `Pin<Box<dyn Future + Send + 'a>>`. For `ScreenRepository` and
`MailStores` this keeps them object-safe behind `Arc<dyn …>` (they are
genuinely held as `Arc<dyn …>`). `Terminal` is the odd one out — it is
**always** a generic `T: Terminal` bound (there is no `dyn Terminal`
anywhere in the tree), so it carries the boxed-future alias only for
shape consistency and is monomorphised at every call site.

The `Terminal` port offers `write` and `flush` (raw byte IO),
`read_line` (one line under an echo policy + timeout), `read_key` (one
single hotkey, no echo — the caller owns every visible byte; the
NextScan pager drives its `More?`/ns-confirm/flag prompts off this,
slice D2b), and `ansi_colour`/`set_ansi_colour` (the `M`-toggle colour
state the `ColourTerminal` decorator reads).

### Build-time provenance

`rust/build.rs` captures the short git SHA (`git rev-parse --short HEAD`)
into the `NEXTEXPRESS_GIT_SHA` compile-time env var. The connect banner
(`app::session_driver::COPYRIGHT_LINES`) and the startup log line emitted by
`bootstrap::run` (via `startup_version_line`) both embed the SHA so operators can pin a running process to
a source commit. The build script falls back to `unknown` outside a
working tree.

### Composition diagram

```mermaid
flowchart LR
    Main["src/main.rs"] --> AppRun["bootstrap::main / bootstrap::run"]

    AppRun --> Config["app::config + config_loader"]
    AppRun --> Seed["app::seed (sysop bootstrap)"]
    AppRun --> ConfRepo["FileConferenceRepository::load_all"]
    AppRun --> UserRepo["UserRepository\n(InMemory or SQLite\nper config.user_storage)"]
    AppRun --> FlaggedRepo["FlaggedStore\n(InMemory or SQLite\nper config.user_storage,\nslice D5-persist)"]
    AppRun --> Hasher["Pbkdf2PasswordHasher"]
    AppRun --> CallerLog["InMemoryCallerLog"]
    AppRun --> Screens["SharedScreens\n(FileScreenRepository)"]
    AppRun --> MailRegistry["InMemoryMailStores\nregistry"]
    AppRun --> FileCatalogue["InMemoryFileRepository\n(seed::demo_file_catalogue,\nSQLite store = slice D2s)"]
    MailRegistry --> FileMailStore["FileMailStore\n(per conference/msgbase)"]

    AppRun --> Runtime["app::runtime::Runtime"]
    Runtime --> NodePool["NodePool"]
    Runtime --> Services["AppServices\n(plain pub-field struct)"]

    Services --> Sharedhasher["SharedHasher"]
    Services --> SharedRepo["SharedUserRepo"]
    Services --> SharedLog["SharedCallerLog"]
    Services --> SharedScreens
    Services --> SharedConfs["SharedConferences\n(Arc&lt;Vec&lt;Conference&gt;&gt;)"]
    Services --> SharedMail["SharedMailStores"]
    Services --> SharedFiles["SharedFileRepo"]
    Services --> SharedFlagged["SharedFlaggedStore\n(Arc&lt;dyn FlaggedStore&gt;)"]
    Services --> Policy["SessionPolicy / DefaultRatio\nNewUserGateConfig / bbs_name"]

    AppRun --> Telnet["TelnetListener::bind"]
    Telnet --> Colour["ColourTerminal\n(decorator: strips ANSI\nwhen M-off)"]
    Colour --> Terminal["TelnetTerminal"]
    Telnet --> Driver["SessionDriver\n(per connection, drives the\nColourTerminal)"]

    Driver --> Start["start (banner + copyright)"]
    Driver --> Login["LoginFlow"]
    Driver --> Registration["RegistrationFlow"]
    Driver --> AutoRejoin["auto-rejoin resolution\n(inline in run; + logon conference scan, L1)"]
    Driver --> Menu["MenuFlow"]
    Driver --> Finalise["session_flow::enter_menu /\nfinalise_logoff"]

    Login --> Typed["domain::session::typed\n(phase wrappers)"]
    Registration --> Typed
    Menu --> Typed
    AutoRejoin --> Typed
    Login --> SF["session_flow\n(typed-only use cases over ports)"]
    Registration --> SF
    Driver --> Presenter["session_presenter\n(menu-prompt / join / stats renderers)"]
    Driver --> WireText["wire_text\n(shared cross-cutting primitives:\nCRLF, ANSI prompt, idle/logon goodbyes,\ninvalid-message-number notice)"]

    Menu --> Parse["menu_command::parse_menu_command"]
    Parse --> Cmds["MenuCommand (25 variants)\n{Logoff, Join, JoinMsgBase, Read, ScanAllMail,\nPost, CommentToSysop,\nShowTime, ShowVersion, ShowHelp,\nQuietToggle, ShowStats, ExpertToggle,\nShowMenu, TopicHelp, AnsiToggle,\nConferenceFlags,\nPrevConference, NextConference,\nPrevMsgBase, NextMsgBase,\nFileList, ZippySearch, AlterFlags, Unknown}"]
    Menu --> MenuFlowHandlers["menu_flow/*\n(one module per command:\nterminal-free core + handler +\nthat command's own wire text)"]

    MenuFlowHandlers --> ReadSub["read_subprompt loop\n(legacy readMSG: CR/A/R/F/\nD/M/EH/L/Q options)"]
    MenuFlowHandlers --> BaseHelpers["menu_flow shared helpers\n(current_base, lock_current_base,\nallowed_addressing_for)"]
    MenuFlowHandlers --> MailText["menu_flow::mail_text\n(shared mail-family text:\nno-mail / store-error / post-* lines,\nrender_post_success)"]
    MenuFlowHandlers --> Table["menu_flow::table\n(shared column helpers:\nleft_field, scan_row_status)"]
    BaseHelpers --> MailRegistryPort
    MenuFlowHandlers --> FileList["file_list\n(NextScan F lister: dir_row + wire\n+ 29-line ScanState pager;\nplus the internal Z zippy search, slice D4)"]
    FileList --> FilePort["FileRepository (port)"]
    FileCatalogue -.implements.-> FilePort
    MenuFlowHandlers --> Rules["domain::messaging::*\n(post / read / scan / reply / forward /\nkill / move / edit_header / comment / attach_file)"]
    ReadSub --> Rules

    Rules --> Mail["domain::Mail"]
    Rules --> Pointers["domain::ReadPointers"]
    Rules --> MailPort["MailStore (port)"]
    SF --> DomainSession["domain::Session"]
    SF --> DomainUser["domain::User"]

    ConfRepo -.implements.-> ConfPort["ConferenceRepository"]
    UserRepo -.implements.-> UserPort["UserRepository"]
    Hasher -.implements.-> HasherPort["PasswordHasher"]
    CallerLog -.implements.-> LogPort["CallerLogAppender"]
    SharedScreens -.implements.-> ScreenPort["ScreenRepository\n(app port)"]
    MailRegistry -.implements.-> MailRegistryPort["MailStores\n(app port)"]
    FileMailStore -.implements.-> MailPort
    FlaggedRepo -.implements.-> FlaggedPort["FlaggedStore (port)"]
    SharedFlagged --> FlaggedPort
    Terminal -.implements.-> TermPort["Terminal\n(app port)"]
    Colour -.implements.-> TermPort
```

### Phase-typed session

`domain::session::typed` lifts the phase enum into eight wrapper types so
the wrong handle for a given transition becomes unrepresentable at
compile time:

`ConnectingSession` → `IdentifyingSession` → `AuthenticatingSession` →
(`NewUserRegisteringSession`) → `OnboardedSession` → `MenuSession` →
`LoggingOffSession` → `EndedSession`.

A ninth construct, the `ActivePhase` enum (`typed.rs:449`), folds the
four readable phases (`Identifying`/`Authenticating`/
`NewUserRegistering`/`Menu`, but not `Onboarded`, which the driver
passes straight through) together so the cross-phase idle-timeout and
carrier-loss handlers can take one value and return a
`LoggingOffSession`.

`SessionDriver::run` threads these wrappers across the sub-flows. There
are **no** mail/messaging transitions on `Session` (the narrowing
refactor removed them): the menu use cases obtain `&mut User` via
`MenuSession::user_mut()` and call the `domain::messaging::*` rules
directly, so the typed module imports no messaging rules and stays
focused on the state machine rather than acting as a command registry.

### Application services container

`app::runtime::Runtime` is the single composition point for driven
adapters, policy values, the screen repository, and the `NodePool`. It
holds an `AppServices` value (also `Clone`, `Arc`-backed) that the
listener clones per accepted connection. `AppServices` carries:

| Field | Type |
|---|---|
| `user_repo` | `Arc<dyn UserRepository + Send + Sync>` |
| `hasher` | `Arc<dyn PasswordHasher + Send + Sync>` |
| `caller_log` | `Arc<dyn CallerLogAppender + Send + Sync>` |
| `screens` | `Arc<dyn ScreenRepository + Send + Sync>` |
| `conferences` | `Arc<Vec<Conference>>` |
| `mail_stores` | `Arc<dyn MailStores + Send + Sync>` |
| `file_repo` | `Arc<dyn FileRepository + Send + Sync>` (slice D1: file areas + listings for the `F` family; the seeded in-memory demo catalogue until slice D2s lands the SQLite metadata store) |
| `session_policy` / `default_ratio` / `new_user_gate` | `Copy` / small `Arc` |
| `bbs_name` | `Arc<str>` (menu-prompt BBS name) |

The container replaced a borrow-bag that threaded lifetimes through every
flow signature; cloning is now one `Arc` bump per port. After
refactoring 6 there are no accessor methods (no `impl AppServices`, no
`AppServices::new`): the sub-flows take `&AppServices` and read its
fields directly — `services.<port>.as_ref()` for the `Arc<dyn …>` ports
and a plain field read for the `Copy` policy values.

### Menu command surface

`app::menu_command::parse_menu_command` is effect-free. The
`MenuCommand` enum currently covers (with the corresponding handler
module under `app::menu_flow/`):

| Command | Variant | Handler |
|---|---|---|
| `G` / `G Y` | `Logoff { auto }` | dispatch (plain `G` with a non-empty session flag set runs the `checkFlagged` confirm via `confirm_leave_flagged` — the live-captured `Do you leave without them? (y/N)?` single-key `yesNo`, default N → return to menu; `G Y`, a `Y` answer, or an empty flag set log off, slice D5/Ga) |
| `J` / `J <n>` / `J <n>.<b>` / `J <n> <b>` | `Join(JoinArg)` | `join` (direct in-range arg joins; everything else opens the legacy `Conference Number (1-N): ` single-shot prompt; the dotted / two-token forms carry a message-base request, out-of-range bases opening the `Message Base Number (1-N): ` prompt whose answer goes to the join unclamped) |
| `JM` / `JM <b>` | `JoinMsgBase(MsgBaseArg)` | `join` (message base of the current conference; single-base conferences get the legacy "does not contain multiple message bases" notice; missing/out-of-range args open the base prompt, whose answer is clamped — the legacy `J`/`JM` asymmetry; a dotted arg delegates to `J`) |
| `<` / `>` | `PrevConference` / `NextConference` | `join` (nearest granted conference below/above, primary base, skipping revoked; past the edge → the `J` prompt) |
| `<<` / `>>` | `PrevMsgBase` / `NextMsgBase` | `join` (current base ∓ 1; past either edge → the `JM` no-arg flow) |
| `R` / `R <n>` | `Read(NumberArg)` | `read_mail` → `read_subprompt` (bare `R` = prompt-first at the read-pointer resume point; `R <n>` = read-first; the `RP`/`FW`/`K`/`MV`/`EH` verbs live inside the sub-prompt, not at the menu — Tier B B8) |
| `E` / `E <to>` | `Post(PostArg)` | `post_mail` (body via `read_editor_body` — the ruler / numbered-line editor + `Msg. Options:` save menu) |
| `C` | `CommentToSysop` | `post_mail` (same ruler editor) |
| `T` | `ShowTime` | dispatch (`render_time_line`) |
| `VER` | `ShowVersion` | dispatch (`VERSION_BANNER`) |
| `H` | `ShowHelp` | dispatch (`bbs_help_screen` asset) |
| `Q` | `QuietToggle` | dispatch (`toggle_quiet_mode`) |
| `S` | `ShowStats` | dispatch (`render_stats_screen`) |
| `X` | `ExpertToggle` | dispatch (`toggle_expert_mode`; gates menu display) |
| `?` | `ShowMenu` | dispatch (`render_menu_screen`, expert mode only) |
| `^<topic>` | `TopicHelp(String)` | dispatch (`screens().topic_help`) |
| `M` | `AnsiToggle` | dispatch (`terminal.set_ansi_colour`; `ColourTerminal` strips ANSI when off) |
| `MS` | `ScanAllMail` | multi-conference mail scan — `scan_all_mail`; per base with matched mail, offers `Would you like to read it now` and (on Yes) attaches that base as a transient read visit and drops into `read_subprompt`, restoring the home conference after |
| `CF` | `ConferenceFlags` | `conf_flags` — the M/A/F/Z scan-flag editor (legacy `internalCommandCF`); redraws the listing, reads a mask key then a conference expression (`+`/`-`/`*`/list) and applies it to the caller's own `ConferenceMembership` flags via `domain::conference_flags`. Gated on `Right::EditConferenceFlags`. |
| `F` / `F <dir>` / `F A`/`U`/`H` / `… NS` / `F ?` / `FR …` | `FileList(FileListArg)` | `file_list` — the NextScan lister (Tier D D1+D2; parity target is the AquaScan door the stock deployment shadows `F` with, NextScan-branded — `comparison/evidence-tierD/live-observations.md`). The `FR` reverse token (slice D3) reuses this code path via a `reverse` flag on `FileListArg::Span`: banner `'fr ?'` (dash run flexed 40→39), `Reverse-scanning dir N... Ok!` header, files newest-first, multi-dir spans descending; bare `FR` opens the `Directories:` prompt under the reverse banner (symmetric with bare `F`), then reverse-walks the chosen span — following `express.e:27645` over the AquaScan capture (which skips the prompt for `FR`). `dir_row` renders the legacy upload-writer row layout from `File` fields; `wire` holds the capture-pinned `&[u8]` constants (banners, separator art, prompts, in-pager help, `F ?` screen) and the date-group frame assembler; the module-local `ScanState` pager pages at 29 lines with the captured `More?` verb set (`Y`/`n`-hold/`ns`+confirm/`C`/`F`/`R`/`?`/`Q`) over true single-key hotkey reads (`Terminal::read_key`, slice D2b; held-`n`/Enter and bare-LF corners probe-pinned). `F`/`R` flag listed files into the session's `FlaggedFiles` set (slice D2f), rendered as an on-row `[X]` marker and repainted in place when ANSI is on; `ScanState` carries the scan-wide `listed` registry the flag verbs match against. Reads `services.file_repo` only — listings are generated at runtime; no DIR files on disk. |
| `Z` / `Z <token>` | `ZippySearch(ZippyArg)` | `file_list::handle_zippy_search` — the internal zippy text search (Tier D D4; `internalCommandZ`, `express.e:26123`). **Not** AquaScan-shadowed, so parity is the genuine internal command — captured live (`comparison/transcripts/ae_tierd_zippy{,2}.txt`). Emits plain text (raw `dir_row` rows, **no** NextScan frames/colour): the `Enter string to search for:` prompt (bare `Z`), the internal `getDirSpan('')` `Directories: …=none? ` prompt (distinct from the AquaScan `=None ?` form), `Scanning directory N` headers, and the `No such directory.` error. The prompt's number/`U`/`A`/`H`/none/out-of-range answers are honoured. The inline `item(1)` form `Z <term> <span>` (slice D7) resolves the span with the same `getDirSpan` logic but **without** the prompt — `Z ART 1` scans immediately (`ZippyArg::QueryInDir`); large-match pagination still defers. Matching is `UpperStr`+`InStr` over each rendered row (filename included) — a hit dumps the whole block. Reads `services.file_repo` only. |
| `A` | `AlterFlags` | `handle_alter_flags` — the genuine internal `alterFlags` -> `flagFiles` loop (Tier D D6a/D6b; `express.e:24601`, `:12648`, `:12594`). **Not** AquaScan-shadowed, captured live (`comparison/transcripts/ae_tierd_alterflags.txt`). Each pass renders the `showFlags` listing (`No file flags` or the upper-cased flagged names space-joined, `render_flag_listing`) then the `Filename(s) to flag: (F)rom, (C)lear, (Enter)=none? ` prompt (`FLAG_PROMPT`). A typed name flags via `flag_add` (`addFlagToList`, no on-disk existence check, current conference + area `0`) and exits to the menu with no trailing line; bare `C` opens the `Filename(s) to Clear: (*)All, …` sub-prompt (`CLEAR_PROMPT`) where `*` clears the whole set and re-prompts; `<CR>`=none ends the loop. The `FlaggedFiles` set is the same session aggregate the `F`/`R` pager verbs build. Deferred: `F`-from (`flagFrom`), clear-by-name (`removeFlagFromList`), the `ACS_DOWNLOAD` gate. |
| anything else | `Unknown` | dispatch (`UNKNOWN_COMMAND_LINE`) |

`read_subprompt` is the legacy `readMSG` sub-prompt loop (Tier B). `R <n>`
and the `MS` read-it-now flow enter it read-first (display the message,
then loop with the pointer past it); bare `R` enters prompt-first at the
read-pointer resume point and reads the first message on the first `<CR>`
(slice B10). The range lower bound is the next message to read and
collapses to `( QUIT )` once the pointer passes the last message; `<CR>`
walks forward to the next message and `Q` returns to the menu. The
message-action options dispatch to their existing rules, preserving the
legacy post-action navigation: `A`gain re-displays (stays), `R`eply
advances, `F`orward stays, `D`elete advances (gated by
`delete_mail::can_delete`), `M`ove advances on success only (gated by
`move_mail::can_move`), `EH` edits the header then re-displays (gated by
`edit_mail_header::can_edit_header`). `?` / `??` render the short / long
help list (gated the same way), and `L`ist shows the legacy `listMSGs`
table (start-message prompt, addressed-to-reader rows via
`menu_flow/list_messages`) paginated through the shared `menu_flow::pager`
(`(Pause)...More(y/n/ns)?`). The surface is modelled in
`messaging.allium:MailReadPrompt`. The three access gates currently
diverge from the legacy `ACS_*` flags — tracked as Tier B slice B9.

Each non-trivial command lives in **one module** under
`app/menu_flow/*`: a terminal-free core fn (plus its outcome enum)
that resolves stores/repositories and returns an outcome, followed by
the `impl MenuFlow` handler that owns the prompts and wire rendering.
The terminal-free seam is the core fn's *signature* (it never takes a
`Terminal`), which is what the unit tests drive with in-memory stores;
a separate core fn is added only when there is real store/repository
resolution to keep terminal-free — never to ceremonially forward a
domain transition. The one outsized command, `MS`, keeps its walk in a
`scan_all_mail/core.rs` submodule. Adding a new command means adding a
module under `app/menu_flow/`, a `MenuCommand` arm, and (usually) a
domain rule. It must also be advertised in the main menu: the
`main_menu_advertises_exactly_the_implemented_commands` test pins
`Conf02/Menu5.txt` against the `MenuCommand` set via an exhaustive
`advertised_token` match, so a new variant fails to compile until it is
given a menu token and the assertion then fails until the menu asset
lists it (simple toggles/queries are otherwise handled inline in
`MenuFlow::dispatch` rather than in their own module).

### Driver and sub-flow split

`TelnetListener` only binds, accepts streams, runs the IAC negotiation,
and constructs a per-connection `SessionDriver`. `SessionDriver` is a
thin orchestrator:

1. `start` — write banner + copyright, return an `IdentifyingSession`.
2. `LoginFlow::identify` — ask the graphics question (`ANSI_PROMPT`;
   `n`/`N` turns the terminal's live colour mode off so screens render
   with ANSI stripped), then prompt for name, dispatch to register,
   verify password, return
   `Onboarded | LoggingOff | Ended | Aborted | NeedsRegistration`
   (`Aborted` is the post-password save-failure outcome added by the
   June-2026 don't-panic fix; the driver turns it into a clean close).
3. `RegistrationFlow::run` — only on `NeedsRegistration`. Owns the
   new-user gate, profile collection, hash + persist, returns
   `Onboarded | LoggingOff`.
4. Auto-rejoin resolution (inline in `run`) — apply
   `conferences.allium:JoinConference`, attaching the home visit and
   **capturing** the `JOINED` announcement (it is replayed in step 6,
   after the logon scan — the legacy emits it at
   `SUBSTATE_DISPLAY_CONF_BULL`, after `confScan`). No join scan fires
   here: the legacy auto-rejoin carries `FORCE_MAILSCAN_SKIP` because
   the logon scan (step 5b) covers every flagged base.
5. `enter_menu` then **logon conference scan** (L1) —
   `MenuFlow::run_logon_conference_scan` runs the legacy `confScan`
   before the menu: the same multi-conference `scan_all_mail` walk the
   `MS` command renders (header, per-conference banner, listing, and the
   read-it-now offer), but filtered to `mail_scan`-flagged bases
   (`ScanFilter::MailScanFlagged`) and skipped on a quick logon. The
   driver then **replays** the captured auto-rejoin `JOINED` + name-type
   promotion and renders the user-stats screen (`render_stats_screen`,
   post-`enter_menu` so `times_called` reflects the logon bump).
6. `MenuFlow::run` — the command loop above, returns `LoggingOffSession`.
7. `finalise` — apply `session_flow::finalise_logoff` and persist.

Rendering helpers shared by the auto-rejoin and explicit-join paths
(`render_menu_prompt`, `auto_rejoin_line`, `explicit_join_line`,
`render_stats_screen`) live in `app::session_presenter`. Each command's
own user-facing text lives beside the module that emits it under
`app::menu_flow/*`; the connect banner's `COPYRIGHT_LINES` (and
`NO_CONFERENCE_ACCESS_LINE`) live in `app::session_driver`, the
login/registration/password-reset text in the respective flow modules,
and `app::wire_text` keeps only the handful of cross-cutting primitives
no single command owns.

### Phase 6–8 messaging behaviour

The messaging rule family is wired end-to-end. The domain rules stay
pure; the app layer resolves the per-msgbase `MailStore` handle through
the `MailStores` registry service
(`services.mail_stores().for_msgbase(...)`), locks it, threads it into
the rule, and writes the legacy ANSI output.

- **Phase 6 (read)**:
  - `domain::Mail` (Slice 37) plus the `MailStore` port. `FileMailStore`
    writes one JSON file per message at `<msgbase-dir>/<seven-digit
    zero-padded number>.json`, scans the directory at open time to
    recover `highest_message`, and holds the spec's
    `lock_msgbase(msgbase)` predicate as an in-process
    `tokio::sync::Mutex`. Timestamps on the wire (`posted_at`,
    `received_at`) are RFC 3339 strings in UTC via the `time` crate's
    `serde-well-known` adapter.
  - `domain::ReadPointers` (Slice 38) attached as a `Vec` on every
    `ConferenceMembership`. `read_pointers_for(user, msgbase)` is the
    spec's black box; rows are lazily created on first
    `ReadMail`/`ScanMail` for a base.
  - Slices 39–41 wire `read_mail`, `scan_mail` and the join scan. The
    `R <num>` handler does the `MailStore::load` → `read_mail` →
    `MailStore::save` dance; `MS` walks the stores via `scan_mail`
    (bare `M` is the ANSI toggle since the A8 rebind); the
    explicit-join path fires `scan_mail_on_join` (inlined beside the
    `J` handler in `menu_flow/join.rs`). The auto-rejoin no longer
    scans on join (L1): the logon conference scan covers every flagged
    base just before the menu opens.
  - Slice 41a wires the file-backed registry into the composition root:
    `bootstrap::run` walks the loaded conferences and opens one
    `FileMailStore` per `(conference, msgbase)` coordinate.

- **Phase 7 (write)**:
  - Slice 42: `domain::post_mail` plus the `E` / `E <to>` handler. The
    rule allocates the next number via the store, persists, and bumps
    the user-level `messages_posted` and per-membership counters.
  - Slice 43: `AllowedAddressing` / `AllScanScope` land as
    `[[msgbase]]` fields. `domain::mail::addressing_allows` is the
    permission black box; `post_mail` enforces it; the `E` handler
    normalises `ALL` / `EALL` / empty before the rule sees them.
  - Slice 44: `domain::post_comment_to_sysop` reuses `post_mail::apply_post_mail`
    so users with `CommentToSysop` but not `EnterMessage` can post. The
    recipient resolves through `UserRepository::find_sysop`.
  - Slice 47: `User.censored` and the visibility downgrade
    (censored → `PrivateToSysop`, EALL → `Public` still wins).

- **Phase 8 (advanced + sysop ops)**:
  - Slice 45 `reply_to_mail`, Slice 46 `forward_mail`, Slice 48
    `attach_file_to_mail` (with the `domain::bytes::Bytes`
    newtype), Slice 49 `delete_mail` / `edit_mail_header` /
    `move_mail`.
  - Slice 49a / 49b wire `RP`, `FW`, `K`, `MV`, `EH` through
    `menu_flow/{reply_forward, sysop_admin}` (terminal-free cores +
    handlers in the same modules). `tests/phase7_smoke.rs` /
    `phase8_smoke.rs` drive the compiled binary end-to-end over
    telnet.

### User storage

The composition root picks the user-repository adapter from
`config.user_storage`:

- `None` → `InMemoryUserRepository`. Always seeds the default sysop.
  Data is lost on shutdown. Default for `cargo run` against a fresh
  tree, and the default for every test.
- `Some(path)` → `SqliteUserRepository::open(path)`. Three tables:
  `users` (single-valued fields), `conference_memberships` (joined to
  `users`), `read_pointers` (joined to memberships). Schema laid out in
  `designs/USERS.md`. Round-trips through the domain's
  `PersistedUser` snapshot.

Seeding the default sysop runs only when the chosen store is empty
(`SqliteUserRepository::is_empty`), so restarting against an existing
database preserves on-disk state. `tests/sqlite_user_storage_smoke.rs`
covers two-boot persistence with a `tempdir`.

### Concentration-of-responsibility hotspots

The current top files by line count (figures re-measured June 2026,
after the slice-13 directory-module promotions and the wire_text
migration — refactoring 9 below). The largest files are
now **test** modules: the inline test blocks of `file_list`, `join` and
`telnet_listener` were carved out to sibling `tests.rs` files
(refactoring 13), so each command/adapter's production `mod.rs` is small
(`file_list/mod.rs` 626, `join/mod.rs` 605, `telnet_listener/mod.rs`
214) while its co-located test sibling rises to the top. `app/wire_text.rs`
no longer appears: the per-command text it once accumulated now lives
beside each command (refactoring 9 below), leaving it at 36 lines of
shared cross-cutting primitives.

| File | Lines | Notes |
|---|---|---|
| `app/menu_flow/file_list/tests.rs` | 2285 | NextScan lister tests, carved out as a sibling of `file_list/mod.rs` (626 production lines) by refactoring 13. |
| `domain/session/tests.rs` | 2062 | Cross-capability session tests in 14 nested mods, internally grouped but monolithic. |
| `app/menu_flow/join/tests.rs` | 1615 | `J`/`JM`/`<`/`>`/`<<`/`>>` family tests, sibling of `join/mod.rs` (605 production lines + the inlined `scan_mail_on_join`); refactoring 13. |
| `adapters/telnet_listener/tests.rs` | 1574 | In-process integration tests (44 fns) for `TelnetListener`/`TelnetTerminal`, sibling of `telnet_listener/mod.rs` (214 production lines); refactoring 13. |
| `app/menu_command.rs` | 1532 | `parse_menu_command` if-chain + the 24-variant `MenuCommand` enum + the parse/reject test battery + the `advertised_token` safety net. |
| `domain/user/mod.rs` | 1527 | `User` aggregate, cross-VO invariants, co-located tests. Private value objects now live in sibling files (`account_status.rs`, `conference_access.rs`, `credentials.rs`, `profile.rs`, `ratio_policy.rs`, `usage_accounting.rs`) plus the public DTOs (`draft.rs`, `persisted.rs`). |
| `app/session_flow.rs` | 1496 | Login-path use cases over the phase wrappers + `(UserRepository, PasswordHasher, CallerLogAppender)` plus the registration-flow facade (refactoring 5 deleted the twin layer). |
| `app/session_driver.rs` | 1390 | Per-connection orchestrator + logon-order tests; now also owns `COPYRIGHT_LINES` / `NO_CONFERENCE_ACCESS_LINE` (wire_text migration). |
| `adapters/file_mail_store.rs` | 1350 | Per-msgbase JSON store + lock + tests. |
| `adapters/sqlite_user_repository.rs` | 1205 | Schema init + row codec + queries + ~30 tests. Flat-file `tests.rs` promotion candidate (refactoring 13). |
| `adapters/file_screen_repository.rs` | 1019 | File-backed screen assets with caching + tests. Flat-file `tests.rs` promotion candidate (refactoring 13). |
| `domain/messaging/scan_mail.rs` | 941 | Scan rule + extensive test fixtures. |
| `app/menu_flow/file_list/wire.rs` | 920 | Capture-pinned `F`-family wire constants + the date-group frame assembler (refactoring 9 colocation). |
| `domain/conference.rs` | 896 | `Conference`, `MessageBase`, `ConferenceMembership` (incl. the M/A/F/Z `ScanFlag` accessors), `NameType`, `AllowedAddressing`, `AllScanScope`. The `CF` edit semantics live in the focused `domain/conference_flags.rs`. |
| `domain/messaging/post_mail.rs` | 886 | Post rule + helpers + tests. |
| `app/menu_flow/post_mail.rs` | 789 | The `E`/`C` editor command module (core fns + editor handlers + co-located mail-entry/editor wire text + tests). |
| `domain/session/typed.rs` | 650 | Phase-typed wrappers and their constructors. |
| `app/menu_flow/mod.rs` | 631 | `MenuFlow` dispatch + shared base helpers + the menu-command consts colocated by the wire_text migration (`VERSION_BANNER`, toggle lines, `GOODBYE_LINE`, `render_time_line`, …). |

## Idiomatic-Rust read

What is already idiomatic:

- **Domain ports are sync, application ports async.** The domain has
  zero `tokio::*` references; `async` lives at the boundary
  (`Terminal`, `ScreenRepository`, `MailStores`). This makes the rules
  trivial to test with stack-allocated stores and keeps `await`
  pressure on the I/O side.
- **Hexagonal invariants are enforced by an integration test**, not by
  convention. The infrastructure-reference guard catches the leak shape
  most projects miss (`source: serde_json::Error`).
- **Cheap-clone services container** (`AppServices`). Each port is held
  behind `Arc`, so per-session clone is a fixed cost and no lifetimes
  leak into flow signatures.
- **Phase-typed session wrappers**. Eight wrappers turn "session is in
  state X" assertions into compile errors.
- **Tight value-object grouping inside `User`** — six private structs
  group related fields; two of them (`Credentials`, with
  `SaltMatchesAlgorithm`, and `AccountStatus`, with
  `LockoutClearsAttempts`) enforce their own invariants in their
  constructors, the other four are plain field bundles.
- **`thiserror` enums everywhere**, with `#[from]` only where the
  conversion is unambiguous. `Box<dyn Error + Send + Sync>` is used at
  the binary entry point and as the type-erased `#[source]` on two
  domain port errors (`StoreSourceError` on `MailStoreError`,
  `ConferenceRepositorySourceError` on `ConferenceRepositoryError`).
  That is the intentional opaque-source idiom, not an infrastructure
  leak: no concrete I/O type appears in the domain (`std::io::Error` was
  removed from both in the June 2026 error-boundary pass and is now
  guarded against).
- **Effect-free parsers** (`menu_command`) decoupled from the dispatch
  loop. `parse_menu_command` is pure; reasonable to fuzz.
- **`#![forbid(unsafe_code)]` plus clippy pedantic at warn level.**

What is less idiomatic and worth flagging:

- **`NameLookupResult::Found(Box<User>)`** boxes the resolved record to
  keep the enum small. Sensible (User is ~2 KB) but ad-hoc.
- **Production `ConferenceVisit` resolution is already clean.** (An
  earlier note here claimed "six `panic!` accessors" — that was wrong on
  every count: there are three `panic!`s, all inside `#[cfg(test)]`
  helpers (`assert_resolved`/`assert_granted`, test mod starts at line
  339), and the production accessors at `conference_visit.rs:64-97` never
  panic. The resolvers return data enums — `JoinResolution{Resolved|NoAccess}`,
  `ExplicitJoinResolution{Granted|Denied}` — that callers match
  exhaustively, so the `ResolvedVisit`/`PendingVisit` type-state idea
  solved a non-problem. Bullet retained only to record the correction.)
- **`Pin<Box<dyn Future + Send + 'a>>` boilerplate** on `Terminal` and
  `ScreenRepository`. With Rust 1.75+ `async fn` in trait, the
  `Terminal` trait could shed the alias (`Terminal` is already generic
  at call sites — there is no `dyn Terminal`); `ScreenRepository` would
  need `async_trait` or the `RPITIT` variant because it lives behind
  `Arc<dyn …>`. The boilerplate is overwhelmingly in **test** code:
  there are 11 `impl Terminal` sites, **9 of them `#[cfg(test)]`
  doubles**, carrying ~29 `Box::pin` wrappers vs ~6 in production. So
  the win is in writing the consolidated capture-terminal double once
  without `Box::pin` — fold the conversion into refactoring 12 (which
  rewrites those exact impls), not as a standalone change. (`Send` must
  still hold at the `tokio::spawn` boundary, which it does because the
  spawn resolves at the concrete `ColourTerminal<TelnetTerminal>`.)
- **`std::sync::Mutex::lock().expect("…")`** in three adapters
  (`SqliteUserRepository`, `InMemoryUserRepository`,
  `InMemoryCallerLog`). Panic-on-poison is acceptable here, but the
  duplication suggests a thin helper.
- **`eprintln!` for operational logging** in `file_mail_store.rs` (1
  call) and `sqlite_user_repository.rs` (5 calls);
  `file_conference_repository.rs` has none. No structured logging or
  level control. Acceptable while there's no `tracing` dependency, worth
  revisiting before more adapters land.
- **Bespoke TOML mirror enums** (`NameTypeToml`, `AllowedAddressingToml`,
  `AllScanScopeToml`) in `file_conference_repository.rs`. Each exists
  only to satisfy serde's snake_case deserialization. A
  `serde(rename_all = "snake_case")` attribute directly on the domain
  enums would remove the mirrors — but that couples domain types to
  serde, which the architecture test would (correctly) reject. The
  mirrors are the right tradeoff; just noting them as boilerplate.

## Large-scale refactorings worth considering

The list below focuses on system-boundary improvements rather than
naming or small local cleanups. It skips refactorings already landed,
including the `domain/user/` value-object split, the repository
`create_user(NewUserDraft)` shape, the bootstrap/app split (a
dedicated `bootstrap` module owns adapter construction; the `app`
module is forbidden from importing `crate::adapters` in production
code, enforced by `tests/architecture.rs`), the mail-store
registry's locking API (the trait now exposes `lock(msgbase) ->
MailStoreGuard` and `lock_pair(source, target) ->
MailStorePairLockOutcome`; the raw `Arc<tokio::sync::Mutex<_>>` is
gone, and `lock_pair` centralises lock ordering and detects
same-store requests before acquiring a second lock), and the
session-typed narrowing (`domain::session::typed` no longer imports
any messaging rules; the per-command `read_mail`/`post_mail`/etc.
methods on `MenuSession` are gone. `MenuSession` now exposes only
state-machine and phase concerns as inherent `pub(crate)` methods (e.g.
`current_msgbase` (`typed.rs:287`), `user_mut` (`typed.rs:264`)) — none
of them per-command messaging operations —
and the menu use cases under `app/menu_flow/*` call the
`domain::messaging::*` rules directly with `session.user_mut()`).

Items 3–12 come from a multi-lens design assessment (June 2026): five
independent review lenses (command-dispatch friction, idiomatic Rust,
hexagonal boundaries, duplication, structural simplicity), with every
suggestion adversarially verified against the code before inclusion.
LOC figures are the verifier's adjusted estimates, not the finders'
optimistic originals. The headline finding: the add-a-command friction
was accidental, not essential to the hexagon — it came from the
then-parallel `app/menu/` + `app/menu_flow/` trees (now folded into one
by refactoring 3 — there is no `app/menu/` directory today), dead
generality left behind by the L1 refactor, and `wire_text.rs` being a
mandatory stop on every command's tour. Items 3, 4 and 9 together cut
the add-a-command tour from ~6 app-layer touch-points to ~4 (empirical
baseline: the `CF` commit touched 9 files / ~630 lines; `MS` touched
13 files).

Items 14–27 come from a second, **forward-looking assessment
(July 2026)** run against the SLICES.md roadmap: seven parallel
subsystem readers, seven design lenses (ports/boundaries, domain
modelling, concurrency readiness, binary-transport readiness,
persistence evolution, app structure, test strategy), deduplication,
then adversarial per-item verification against the code — 18
candidates survived, 0 refuted. Four of the 18 re-timed existing items
in place (1, 2, 10, 12); the rest are new, under "Forward-looking
review additions" below. The headline finding: the next tiers each
stress a seam that does not exist yet — Tier D transfer needs a
binary-clean transport (item 20) and schema evolution (item 22),
Tier E needs cross-session state (items 25–26; today there is none —
no registry, no channel, and production nodes never leave
`Connecting`), and Tiers G–I need finer-grained user writes than the
whole-aggregate upsert (item 1). The review also surfaced three live
defects (the open-defects list under "Suggested order"). Sizes are the
verifiers' adjusted estimates.

### 1. Evolve user persistence away from full aggregate saves

`UserRepository::save(User)` persists the whole aggregate, and flows
clone the session-bound user back to storage after logon, menu entry,
logoff, read-pointer changes, message-post counters, and account-state
changes. The SQLite adapter responds with a broad upsert over almost
every user column.

That is simple and has worked well for a single active session, but it
becomes a lost-update risk as more mutable user subdomains land or if a
user can be touched by background/sysop actions while logged in.
Consider command-style writes or optimistic versioning for separate
state families: login/account status, read pointers, usage counters,
conference position, profile fields, and password changes.

This does not need to happen immediately. It becomes important before
adding cross-session sysop edits, background maintenance jobs, or
multiple concurrent logons for the same account.

**Re-timed + mechanism decided (July 2026 review): land after N,
before D-T2.** The accepted transfer design (designs/FILES.md's ledger
deltas — `SET bytes_downloaded_total = bytes_downloaded_total + ?`)
applies additive projection deltas that a whole-aggregate save at the
session's next save point silently reverts, so the first real
second-writer path arrives with D-T2, not Tier H. Mechanism:
**command-style writes, not optimistic versioning** — three naturally
commutative commands, each one SQL transaction:
`record_auth_outcome` (invalid attempts / lock / force-reset, at
verify_password), `record_logon` (times_called additive, last_call
monotonic MAX, at enter_menu), and `apply_logoff_patch`
(time_used_today delta, preference/last_joined patch, messages_posted
delta, per-row read-pointer MAX-upserts, at finalise). Versioning is
rejected for now: a `row_version` column cannot be added to existing
users.db files until migrations (item 22) exist, and it turns
commutative counter bumps into retry loops — keep it in reserve for
the Tier H preference-editor UX. The same pass fixes a verified tear:
`save` runs the 32-column upsert + membership DELETE/reinsert **bare**
on the connection while `create_user` gets a transaction
(`sqlite_user_repository.rs:494-511` vs `:513-546`), so a crash
mid-save tears the aggregate. Verified size: 4–6 days, ~10–14 files —
`apply_logoff_patch` needs the domain to yield per-call deltas rather
than absolutes, plus dual-adapter parity tests.

### 2. Rebalance port error boundaries

**Partially landed (June 2026): `std::io::Error` removed from both domain
port errors.** `MailStoreError::Io(std::io::Error)` and
`ConferenceRepositoryError::Io(std::io::Error)` became
`Backend { source: Box<dyn Error + Send + Sync> }`; each file adapter now
owns the `From<std::io::Error>` translation, and `std::io::` joined the
architecture guard's forbidden list to keep it out. The `NotFound` →
`Ok(None)` check moved into the adapter's read helper (at the I/O
boundary, before the error is type-erased).

What remains: `MailStoreError` still carries path strings and the rich
`Malformed`/`Serialise`/`*Mismatch` variants; `ConferenceRepositoryError`
still models TOML/path failures and lives in `domain` even though
conference loading is a startup/configuration concern.

Prefer semantic domain/application errors at port boundaries and keep
adapter-native details in adapter-specific error types or log context.
`MailStore` may still belong near the messaging rules because the rules
need a storage port, but the error shape can be less file-specific.
`ConferenceRepository` is a stronger candidate to move out to
app/bootstrap because runtime rules consume an already-loaded
`Vec<Conference>`, not a repository.

Verified June 2026 (both are real but LOC-modest, not LOC-wins):

- **`ConferenceRepository` move (~−15 to −25 net).** The trait is never
  used polymorphically — `bootstrap.rs:47` is the *only* production
  import, in scope solely so `bootstrap.rs:197` can call `load_all()` on
  the concrete `FileConferenceRepository`; `AppServices` holds
  `Arc<Vec<Conference>>`, not a repository. All five
  `ConferenceRepositoryError` variants are constructed only in the
  adapter. The clean version: make `load_all` an inherent method on
  `FileConferenceRepository`, re-home the error enum into the adapter,
  delete the trait and the `bootstrap.rs:47` import, retarget the three
  domain doc-comment cross-references.
- **`MailStoreError` reshape (≈ LOC-neutral; architectural hygiene).**
  No caller above the adapter destructures any variant — every domain
  rule just `#[from]`-wraps it, and every app consumer only `Display`s
  it (`eprintln!` + a fixed `MAIL_STORE_ERROR_LINE`). The rich
  variants are constructed and matched exclusively inside
  `file_mail_store.rs`, so they can collapse to one opaque port error
  with the file-specific enum moved adapter-private. The `std::io::Error`
  coupling itself is already gone (June 2026, see the refactoring 2
  intro); what this optional further step buys is collapsing the
  remaining path strings and `Malformed`/`Serialise`/`*Mismatch` detail
  out of the domain enum. Expect the rich enum to *reappear*
  adapter-side, so it is hygiene, not reduction. Best done as one
  error-boundary pass alongside the `ConferenceRepository` move.

**Landed (2026-07-03).** The pass shipped in three moves: (1) the
`ConferenceRepository` trait is gone — `load_all` is an inherent
method on `FileConferenceRepository`, the TOML/path error enum lives
adapter-side, and the domain doc comments now state the
ascending-order loader contract without naming a repository type;
(2) `MailStoreError` collapsed to `Backend { source }` +
`MessageMissing` + a path-less caller-contract `MsgbaseMismatch`,
with the rich file diagnostics reborn as the adapter-private
`MailFileError` (verbatim fields and format strings) boxed into
`Backend` — adapter tests downcast through the source chain;
(3) `FlaggedStoreError::Backend(String)` became the boxed-source
`Backend { source }` shape, restoring the diagnostic chain. The
convention D2s inherits: one opaque `Backend` + only contractual
variants. **Explicitly deferred:** `UserRepositoryError::Storage
{ context, message }` is also stringly, but it leaks no adapter
vocabulary and converting it touches every construction site in both
user-repo adapters — fold it into item 1's command-style-writes pass,
which rewrites those adapters anyway. Original notes follow.

**Re-verified + scope widened (July 2026): do the pass before D2s.**
Status before landing: nothing had landed. The convention actually diverges four
ways, not two: `FlaggedStoreError::Backend(String)`
(`domain/files/flagged_store.rs`) is stringly and discards the source
chain, and `UserRepositoryError::Storage { context, message }`
(`domain/user_repository.rs:31-46`) likewise (it leaks no adapter
vocabulary, but a pass claiming to pin one convention must include or
explicitly defer it). Pin the convention as: one opaque
`Backend { source: Box<dyn Error + Send + Sync> }` plus only genuinely
contractual variants (e.g. `MessageMissing`, `MsgbaseMismatch`), rich
diagnostics adapter-private and Display-chained into `Backend`.
Timing: **before slice D2s mints the port family's fourth member** by
copying whichever template it finds — and today the most prominent
template (`MailStoreError`) is the leaky one. No wire impact: app
consumers only emit the fixed `MAIL_STORE_ERROR_LINE`. Verified size:
~1 day, realistically 12–15 files (doc-comment retargeting in
conference.rs / conference_visit.rs / conferencing.rs / join /
services.rs, ~15 file_mail_store test assertions reworked against the
adapter-private enum, the composition diagram above).

### 3. One module per menu command: fold `app/menu/*` into `app/menu_flow/*`

**Landed** (one command per commit: J, R, E/C, RP/FW, K/MV/EH, MS, L).
The `app/menu/` tree is gone. Each command's terminal-free core fn +
outcome enum now sit at the top of its `app/menu_flow/<cmd>.rs` module
(module-private — the handlers were their only consumers), with the
`impl MenuFlow` handler below; unit tests kept calling the core fns
with in-memory stores throughout the move. The terminal-free seam that
matters for TDD is the *function signature* (no `Terminal` parameter),
not the directory boundary.

Two layers were pure ceremony and were deleted outright:
`app/menu/join.rs` (a rewrap of the domain `ExplicitJoinTransition`
into an identically-shaped enum) and the driver's `AutoRejoinResult` +
`resolve_auto_rejoin` repackaging (inlined into `SessionDriver::run`,
which owns the terminal for the `NoAccess` notice;
`AutoRejoinAnnouncement` stays — deferring the `JOINED` line past the
logon scan is real behaviour). The substantive cores (`post_mail`,
`reply_forward`, `sysop_admin`, `scan_all_mail`, `list_messages`) earn
their keep — real lock acquisition, repo lookups, recipient
classification — and survived as terminal-free fns in the merged
modules. The rule going forward: a separate core fn exists to keep
store/repo resolution terminal-free, never to ceremonially forward a
domain transition. `MS` (the outsized one) keeps its walk in a
`menu_flow/scan_all_mail/core.rs` submodule.

Add-a-command touch-points dropped from ~6 to ~4; new files per
command from 2 to 1.

### 4. Delete the pre-L1 scan-on-join generality

**Landed.** `app/mail_scan_on_join.rs` and `app/menu/scan_mail.rs`
existed so scan-on-join could run from either an `OnboardedSession`
(auto-rejoin) or a `MenuSession` (explicit join); since slice L1 the
auto-rejoin path no longer scans, leaving exactly one production
caller. Both modules are deleted: the lock-and-call body lives in a
single `scan_mail_on_join` fn beside the `J` handler
(`menu_flow/join.rs`), with the `JoinScanMode::FollowPointer`
semantics inlined as the `from_message = 0` sentinel — now pinned by a
behavioural test (a broadcast message behind the read pointer must not
re-surface on the next join) instead of the old enum-accessor pins.
The `BoundMenuUser` trait and both impls are gone; `MenuSession` has
an inherent `user_mut` and the menu use cases call it directly
(−160 net lines).

### 5. Merge `session_flow`'s typed/untyped twin functions

**Landed.** Every login-path use case existed twice: an untyped fn
over `&mut Session` (`name_typed`, `verify_password`,
`verify_new_user_password`, `enter_menu`, `finalise_logoff`,
`NewUserRegistrationFlow::complete`) and a typed wrapper in the nested
`typed` module doing `into_inner → call → expect → from_session`.
Production code called only the typed variants. Each pair is now one
function taking/returning the phase wrappers directly; the untyped
twins, the `typed` delegation module, and the now-redundant
`WrongState` guards are gone (the wrong-state test for registration
completion was deleted with them — the wrapper type makes the case
unrepresentable). Tests build wrappers via the existing `pub(crate)`
`from_session`/`into_inner` and assert on the returned transition — a
stronger pin than post-state checks. (`complete_password_reset` still
drives the raw `Session`; its driver path lands with the
password-reset slice.) Net −130 lines; a new flow rule is written
once, not twice, and the per-rule file split (refactoring 13) got
cheaper.

### 6. Make `AppServices` a plain pub-field struct

**Landed.** The 10-positional-argument constructor and ten accessor
methods were ceremony around a bag of `Arc` fields; `services.rs` is
now a documented pub-field struct (~75 lines shorter). Construction
sites use named struct literals (the test fixtures read better than
the positional list did); port reads are `services.<port>.as_ref()`
and `Copy` policy values are plain field reads. Adding a service field
is now the field plus the construction sites — no constructor or
accessor to keep in sync.

### 7. Delete the dead `NumberArg` plumbing in the read-subprompt handlers

**Landed.** `handle_reply` / `handle_forward` / `handle_kill` /
`handle_move_mail` / `handle_edit_header` were called only from
`read_subprompt.rs`, always with `NumberArg::Number`; their
`Missing`/`Invalid` match arms were unreachable and untested. They now
take a plain `u32`; the five match blocks, the `NumberArg` imports in
the read-subprompt handlers, and the orphaned `READ_REQUIRES_NUMBER_LINE`
constant are gone (−76 net lines). `NumberArg` itself is unchanged — it
remains the parser's numeric-argument representation in
`menu_command.rs` (`Read(NumberArg)`, `parse_number_command`).

### 8. Shared current-base helpers for the mail use cases

**Landed.** The `current_msgbase → lock → addressing` preamble was
copy-pasted across the mail command cores (including three
byte-identical `current_msgbase()` resolution copies). Three private
helpers in `menu_flow/mod.rs` — `current_base`, `lock_current_base`
(returns the `(MessageBaseRef, MailStoreGuard)` pair) and
`allowed_addressing_for` — now serve every command module via
`super::`; the existing `NoMailBase` outcome tests keep killing
mutants inside them.

### 9. Colocate command-specific wire bytes with the command module

**Landed (June 2026).** A new command's single-consumer renderers and
prompt constants now live in its own `menu_flow` module (`pub(super)`
items or a private `wire` submodule), not in `app/wire_text.rs`. The
full migration of the pre-existing text followed: every per-command and
per-flow string that had accumulated in `wire_text.rs` (~1746 lines at
its peak) moved to the module that emits it, no string changes — only
relocation, with the bytes-pinned tests moving next to the handler.

The end state:

- **Login / registration / password-reset text** → `app::login_flow`,
  `app::registration_flow`, `app::password_reset_flow`.
- **`COPYRIGHT_LINES` + `NO_CONFERENCE_ACCESS_LINE`** → `app::session_driver`.
- **`render_menu_prompt`, `auto_rejoin_line`, `explicit_join_line`,
  `render_stats_screen`** (and its private `STATS_DATE_FORMAT` /
  `write_stat_line`) → `app::session_presenter`.
- **Per-command text** into each command's `app::menu_flow` submodule
  (`conf_flags`, `read_subprompt`, `scan_all_mail`, `list_messages`,
  `read_mail`, `sysop_admin`, `reply_forward`, `post_mail`, `join`) —
  so the `CF` block and `MS` block that this item once flagged as
  "what remains" moved too.
- **Menu-command consts** (`VERSION_BANNER`, `HELP_UNAVAILABLE_LINE`,
  the `QUIET`/`EXPERT`/`ANSI_COLOR` toggle lines, `UNKNOWN_COMMAND_LINE`,
  `GOODBYE_LINE`, `LEAVE_FLAGGED_CONFIRM`, `YESNO_*`, `render_time_line`,
  `MENU_PROMPT_SUFFIX`) → `app::menu_flow::mod`.
- **Two new shared submodules** absorb the cross-command duplication
  rather than forcing it back into `wire_text`:
  `app::menu_flow::mail_text` (mail-family shared lines —
  `NO_MAIL_BASE_LINE`, `MAIL_STORE_ERROR_LINE`, `POST_ABORTED_LINE`,
  `POST_RECIPIENT_NO_ACCESS_LINE`, `POST_ACCESS_DENIED_LINE`,
  `POST_ADDRESSING_NOT_ALLOWED_LINE`, `SOURCE_NOT_FOUND_LINE`,
  `FORWARD_UNKNOWN_USER_LINE`, `render_post_success`) and
  `app::menu_flow::table` (shared column helpers `left_field`,
  `scan_row_status`, which this item had flagged as needing
  `pub(crate)` widening).
- `app/wire_text.rs` is **slimmed to 36 lines** of five genuinely
  cross-cutting primitives: `CRLF`, `ANSI_PROMPT`, `IDLE_TIMEOUT_LINE`,
  `LOGON_REJECTED_LINE`, `INVALID_MESSAGE_NUMBER_LINE`.

The per-command growth concern is resolved by co-location: `wire_text.rs`
is no longer a mandatory stop on every command's tour, and it no longer
grows ~100–200 lines per command. This was never the skip-listed
"rewriting `wire_text.rs`" — no shared constant changed and no string
changed; the text simply moved to where it is emitted.

### 10. One parameterised line reader + pure outcome-to-bytes functions

Two mechanical de-duplications inside `menu_flow`, no new layer (this
supersedes the earlier "small menu renderer" idea):

- Merge the three near-identical single-line readers
  (`read_required_line`, `read_optional_line`,
  `read_optional_unchanged_line`) into one helper parameterised by
  empty-line meaning and abort-notice policy, keeping thin named
  wrappers at the call sites. The `record_input` idle-clock stamp then
  cannot be forgotten on a new prompt. The **`EH` abort bug** this merge
  would have prevented has since been fixed directly (see below), so what
  remains here is the dedup, not a correctness fix.
- Convert the static error-rendering matches (`sysop_admin.rs`'s
  `render_delete/move/edit_header_error`, the static arms of
  `render_post_outcome`) from async methods into pure
  `fn line_for(err) -> &'static [u8]` functions co-located with each
  handler — unit-testable with a plain `#[test]`, no capture terminal
  or async runtime, and friendly to cargo-mutants. Caveat: the
  `Store(err)` arms carry an `eprintln!` side-effect and `Posted(mail)`
  needs the message number, so only the *static* arms extract; a smaller
  async `match` remains per handler.

> **Fixed (2026-06-23): `EH` edit-header abort bug.**
> `read_optional_unchanged_line` (`sysop_admin.rs:438`) previously
> returned `Ok(None)` for **both** a blank line ("keep current") **and**
> `Eof`/`IdleTimedOut` (abort), while its doc comment claimed a
> three-state `Some(None)`/`Some(Some)`/`None` API the
> `Result<Option<String>, _>` signature could not express. Its caller,
> `handle_edit_header` (`sysop_admin.rs:330`), fed that `Option` straight
> into `edit_mail_header`, where `None` means "keep current" — so an idle
> timeout or dropped carrier during the subject/recipient prompt was
> silently treated as "keep current" and the header edit proceeded
> instead of aborting. The reader now returns the three-state
> `Result<Option<Option<String>>, _>` its doc comment always described,
> and `handle_edit_header` returns early on the abort case. The abort is
> **silent**, matching the legacy `editHeader` (`express.e:11602`), whose
> every prompt does `IF (stat < 0) THEN RETURN stat` with no notice (the
> same convention as the `R`-sub-prompt reply / forward commands, B6).
> Pinned by three `#[tokio::test]`s in `sysop_admin.rs` (subject-timeout
> aborts silently, addressee-timeout aborts silently, blank input still
> keeps-and-proceeds). The reader-merge dedup above remains open.

Verified impact: net production LOC is roughly neutral to −25 (the
finder's −35 to −55 was optimistic — the named wrappers, the
empty-meaning/abort-policy enum, and the residual error `match`es claw
most of it back). Test LOC *grows* (~+40 to +80): the remaining readers
(`read_required_line`, `read_optional_line`) and the static error arms
are still unpinned by any unit test — the `mod tests` added with the
`EH` abort fix covers only `handle_edit_header`'s abort/keep paths — so
the sync byte asserts are net additions. Justify the
slice on the bug fix and mutant-resistance, not the line count. Land
the error-arm `line_for` extraction + asserts as a standalone TDD move;
fold the reader merge into the next slice that touches `post_mail.rs` or
`sysop_admin.rs`.

**Landed (2026-07-03), menu_flow scope.** `MenuFlow::prompt_line(
session, prompt, EmptyMeaning, AbortNotice) -> PromptLine` is the one
line-prompt reader: it stamps `record_input` on every accepted line
(pinned by `flag_prompt_stamps_the_idle_clock`, written first and
watched to fail), with `EmptyMeaning::{Abort, Keep, Verbatim}` and
`AbortNotice::{Silent, MessageAborted}` as the two axes the old copies
differed on. The three named readers (`read_required_line`,
`read_optional_line`, `read_optional_unchanged_line`) are thin
wrappers; the previously UNSTAMPED prompts — the `A` flag loop and its
clear sub-prompt, the `F` `Directories:` prompt, both `Z` zippy
prompts — now route through it (wire bytes unchanged, suite-pinned).
The join/number prompts keep their bespoke no-trim semantics and
already stamped. Item 10's second half landed too: pure
`delete/move/edit_header_error_line` fns (sysop_admin) and
`post_outcome_line` (post_mail) with plain byte-assert pins, plus a
handler-level test after mutants-diff caught the render-nothing gap.
Two accepted equivalent survivors: deleting the handler log arms
(`LookupFailed`/`Rejected(Store)` `eprintln!`s) is wire-identical.
The cross-flow outcome-mapping consolidation stays cut, as decided.
Original notes follow.

**Sharpened (July 2026): land the menu_flow scope before the N and FM
slices.** Two updates. (a) The duplication is six near-identical
readers across the flows once the join/menu variants are counted, not
three, and the `record_input` idle-stamp convention is quietly
fraying — stamped at 17 menu_flow sites but absent from the `A`
flag-loop reads, the `F` `Directories:` prompt, and both `Z` zippy
prompts. The merged reader (`prompt_line(prompt, EmptyMeaning,
AbortNotice)`) should stamp `record_input` internally so the idle
clock cannot be forgotten, and the unstamped prompts should be swept
onto it or documented as intentional. The `EmptyMeaning` enum must
encode join's no-trim/lone-CRLF blank semantics vs post_mail's
trim+notice. (b) N and FM are the next prompt-heavy slices, so this is
this item's own "fold into the next slice that touches these files"
trigger arriving; ~1–1.5 days for the menu_flow scope. The wider
cross-flow Eof/IdleTimedOut outcome-mapping consolidation
(login/registration/password-reset) was reviewed and **cut**: Rust's
exhaustive matching already protects future outcome variants — the
compiler flags every site when Tier G adds `TimeExpired`.

### 11. Declarative command listing in `menu_command.rs` (low priority)

A const table (`&[(&str, ArgSpec)]`) or a ~40-line `commands!` macro
can drive the parser if-chain, `every_menu_command`, and the **ten**
near-identical `*_rejects_extra_tokens` test fns from one listing —
mirroring the legacy dispatch shape (`express.e:28286` splits the line
into `(cmdcode, cmdparams)` then string-matches the code). The
`main_menu_advertises_exactly_the_implemented_commands` safety net
survives either way: `advertised_token` stays as the one exhaustive
match. Scope it to the no-arg/exact-token commands only — the bespoke
`parse_*` helpers (`J`/`JM`/`R`/`E`/`F`, the angle-brackets) and their
**load-bearing ordering** (bare-token `eq_ignore_ascii_case` checks must
precede the greedy prefix parsers — commit `3899bb8` was a mis-binding
fix) stay untouched. Verified at ~−60 to −90 lines, the bulk of it
test-side (the reject-test battery), confined to one file, so do it for
the shape — one row per command beats a 16-branch if-chain — when next
in the file, not as a priority.

### 12. Test-support consolidation

Zero production risk, large test-code wins:

- **`tests/support/` smoke harness.** There are **two** harness
  families, and only one is addressable: the **7 in-process** smokes
  (`cf_conference_flags`, `confnav`, `logon_conference_scan`,
  `quickwins`, `tierb_read_subprompt`, `tierd_file_list`,
  `tierb_mail_scan`) re-roll a byte-identical async helper quintet
  (`write_line`/`drain_until`/`contains`/`end_session`/
  `sign_in_seeded_sysop`) plus a stable bind/spawn tail. The **6
  binary-subprocess** smokes (`phase1/4/6/7/8`, `sqlite_user_storage`)
  use an incompatible sync `Result<_, String>` shape over a spawned
  binary and must stay separate (they deliberately exercise the argv /
  config path, per AGENTS.md item 6). Extract a `tests/support/mod.rs`
  (declared `mod support;`) for the in-process family only, with a
  parameterised `TestBoard` builder carrying optional conferences /
  memberships / file-repo / seeded-mail / config fields (the
  scenario-specific seed prologues stay caller-passed). Verified net
  **−330 to −360** test lines (the earlier −440/−480 figure counted
  both families); every future in-process command smoke starts
  ~100–130 lines smaller.
- **Crate-root `#[cfg(test)] mod test_support`.** The clean win is
  `test_services()` — **4 byte-identical copies** (`menu_flow/mod.rs`,
  `read_subprompt.rs`, `pager.rs`, `reply_forward.rs`) collapse to one
  builder. `CaptureTerminal` is messier than once thought: **6 copies
  in 4 distinct shapes** — only the two identical `output+inputs` pairs
  (`pager`+`reply_forward`, `mod.rs`+`read_subprompt`) cleanly share a
  double; `colour_terminal.rs` (different field name) and the
  63-line `file_list/mod.rs` superset (adds `keys`/`ansi`) realistically
  stay local. Realistic net **−90 to −150** test lines, not −180/−280.
  Pairs well with the `Terminal` RPITIT conversion (write the shared
  double once without `Box::pin`).

Test clarity beats DRY in this codebase: only the scaffolding moves;
scenario-specific assertions stay in the test files.

**Smoke-harness half landed (2026-07-02).** `tests/support` grew
builder knobs on `TestRuntime` (`.with_config` for `Config` overrides
incl. `max_nodes`, `.with_sysop` for seeded-sysop adjustments,
`.with_user(|hasher| …)` for extra users hashed by the runtime's
hasher), plus the generalised `sign_in(addr, handle, password)`,
`end_session_forced` (`G Y`), the keystroke primitives
`write_key`/`read_idle`, and a `drain_until` that panics with distinct
timeout/EOF/read-error messages. All six hold-out smokes migrated
(assertion literals untouched): −609 test lines net. Still deferred
within this item, as planned: the keys-capable CaptureTerminal
promotion (waits for its second consumer, FS or N) and the two-session
primitives (`sign_in_as`, `expect_within`), which stage with the first
Tier E slice. Pre-landing status follows.

**Status (July 2026): the crate-side half landed; the smoke-harness
half remains and is due before the FS smoke.** The late-June `tidy:`
commits consolidated the crate-internal fixtures
(`app/menu_flow/test_support.rs`, the session_driver services helper,
`domain::messaging::mail_store::test_support`). `tests/support/mod.rs`
now exists (quickwins + the phase smokes use it) but models one shape
only: seeded sysop, `max_nodes` hardcoded to 1 (`support/mod.rs:114`),
sysop-only sign-in helpers, and a `drain_until` that collapses
timeout/EOF/IO-error into one panic message. Six in-process smokes
still re-roll a ~110–155-line helper quintet (`tierd_file_list`,
`tierb_mail_scan`, `tierb_read_subprompt`, `confnav`,
`cf_conference_flags`, `logon_conference_scan`), and `tierd_file_list`
alone carries the keystroke primitives (`write_key`/`read_idle`) the
hotkey pager forced. Remaining work (~1 day, test-only): builder knobs
on `TestRuntime` (extra users, max_nodes, Config overrides — a corpus
knob already exists via the `file_repo` field), migrate the six
hold-outs (assertion literals stay put), move `write_key`/`read_idle`
and a generalised `sign_in(addr, handle, password)` into support, and
split `drain_until`'s failure modes into distinct timeout/EOF/error
panics. FS is the next slice to write a smoke — do this first or it
becomes helper-copy #7. Deferred within this item: the keys-capable
CaptureTerminal promotion (wait for its second consumer, FS or N) and
the two-session primitives (`sign_in_as` returning independent
streams, `expect_within(stream, needle, window)`), which stage with
the first Tier E slice.

### 13. Keep file-size refactors opportunistic

The older navigability refactors are still useful, just lower leverage
than the work above:

- **Carve `app/session_flow.rs` into per-rule modules** when the next
  slice would otherwise add to it. Refactoring 5 (which deleted the
  typed/untyped twin layer) has landed, so the split is now smaller.
  Suggested shape:

  ```
  app/session_flow/
    mod.rs              -- re-exports + shared types (NewUserGateConfig, DefaultRatio)
    name_typed.rs       -- + NameTypedFlowError
    verify_password.rs  -- + VerifyPasswordFlowError
    enter_menu.rs       -- + EnterMenuFlowError
    finalise_logoff.rs  -- + FinaliseLogoffFlowError
    registration.rs     -- NewUserRegistrationFlow + Complete* errors
    password_reset.rs   -- complete_password_reset + CompletePasswordResetFlowError
  ```

- **Sibling test files for large modules (convention adopted June
  2026).** When a module's inline `#[cfg(test)] mod tests { … }` grows
  to dominate the file (rule of thumb: test block ≳1000 lines /
  test-dominated), extract it to a sibling `#[cfg(test)] mod tests;` →
  `tests.rs`. This is the form `rust-lang/rust` enforces (via `tidy`)
  and that `domain/session/tests.rs` already used — **not** `foo_test.rs`
  / `#[path]` (which no surveyed large Rust codebase uses). Inline stays
  the default for the ~60 small modules; do not convert them. It is a
  pure code move (net-zero LOC — navigability, not reduction): the
  sibling `mod tests` is still a child of the same parent, so every
  `super::`/`super::super::` path resolves unchanged.

  Mechanics: a module already shaped as a directory (`foo/mod.rs`) takes
  `mod tests;` → `foo/tests.rs` directly; a flat `foo.rs` must first be
  **promoted** to `foo/mod.rs`. The architecture guard
  (`app_does_not_depend_on_adapters_in_production_code`) strips only
  *inline* test modules, so it now also skips files named `tests.rs` or
  under a `tests/` directory (`is_sibling_test_module`, matching `tidy`'s
  name-based rule) — otherwise a sibling test file's adapter-double
  imports would trip it.

  **Landed (one commit each):** `app/menu_flow/file_list/` (2241 → 626),
  `app/menu_flow/join/` (2186 → 605, promoted to a directory),
  `adapters/telnet_listener/` (1793 → 214, promoted; its `#[cfg(test)]`
  `wire_text` import moved into the test file). **Remaining adapter
  candidates:** `adapters/sqlite_user_repository.rs` (1127) and
  `adapters/file_screen_repository.rs` (1019) — same flat-file promote.
  Do these one-per-commit when the file is otherwise quiet (the move
  noises up `git blame`).

## Forward-looking review additions (July 2026)

Items 14–27 target the seams the remaining tiers need (see the
assessment provenance in the section intro above). Each is listed with
its **trigger** — the slice it must precede — because most of them
should NOT land now: the project rule "one field lands with the slice
that first consumes it" applies to seams too. Items 14 and 15 are the
exceptions (a live defect and a 1–2 hour chore).

### 14. Unify flag identity to `(conference, name)` — live defect

**Landed (2026-07-02).** `FlaggedKey` is now `(conference, name)` —
`area` deleted from the struct, all five production construction sites
and both adapters updated; the in-memory store's save-time
normalisation loop reduced to a plain clone;
`assemble_dir_lines`/`stream_dir_body` dropped their now-unused `area`
parameters. Pinned by three new tests (domain identity, the SQLite
save that used to roll back, prompt-flagged files painting `[X]` in
listings). The marker-repaint question needed no FS-UAE session: the
`[X]` marker is a NextExpress-only aid (the AquaScan door has none),
so the only legacy semantics in play is the flag identity itself,
which `express.e:12534` (`isInFlaggedList`) settles as
`(confNum, fileName)`. Original problem statement follows.

`FlaggedFiles` had two competing identities by convention:
listing-driven flags carry the real dir number
(`file_list/wire.rs:294`, sourced from `run_span`) while the
`A`-prompt (`menu_flow/mod.rs:246`), the logon restore
(`mod.rs:387-392`), and both `FlaggedStore` adapters
(`sqlite_flagged_store.rs:104`, `in_memory_flagged_store.rs:41`) build
`area = 0` keys — and `FlaggedKey`'s `Ord` includes `area`
(`domain/files/flagged.rs:10-15`), so the same file can occupy two
`BTreeSet` keys in one session (flag from an `F` listing plus the same
name at the `A` prompt, or restore-then-reflag). Verified
consequences: `names()` duplicates the name in the `A` listing (any
config); `entries()` emits duplicate `(conference, name)` pairs; and
under `user_storage = sqlite` the logoff save's plain `INSERT` under
`PRIMARY KEY (slot_number, conference, name)` hits a PK violation and
the transaction rolls back — **the session's flag changes are silently
lost** (previously persisted rows survive the rollback; the in-memory
store is unaffected because its `save` dedupes through
`FlaggedFiles::flag`).

Fix: drop `area` from `FlaggedKey` so the domain identity equals the
legacy and persisted identity, `(conference, UPPER(name))`. The
documented "restored flags don't repaint the `[X]` marker" limitation
dissolves as a side effect — validate the marker semantics for a
same-named file in two areas against the FS-UAE reference before
re-pinning, and add the restore → re-flag → save round-trip regression
test. ~0.5–1 day: flagged.rs, `flag_add`, file_list wire+mod, both
adapters, the bootstrap seed, tests, and the three docs restating the
repaint limitation (SLICES.md, this file, designs/FILES.md).
**Trigger: now — the defect is live, and D-T2 consumes the flag list
as the default download set.**

### 15. Re-scope the routine mutation gate to diff-vs-main

**Landed (2026-07-02).** `make check`'s final step is now
`$(MAKE) mutants-diff` (working-tree diff vs `DIFF_BASE`, default
`HEAD`); AGENTS.md's workflow step 4 and Before-Committing item 4 both
name `make mutants-diff`; the full sweep is documented on the
`mutants` target as a scheduled/sharded job
(`MUTANTS_ARGS='--shard k/n'`) with `mutants-run.log`/`mutants.out` as
the baseline artifacts. No `exclude_globs` added, per the review's
warning. Original problem statement follows.

`make check` — the target mirroring AGENTS.md's "Before Committing"
checklist — still runs the FULL `cargo mutants` sweep (Makefile:61):
1,882 mutants at an observed ~11–17 s each ≈ 6–9 hours serial. Nobody
runs it; the practiced norm is the per-commit `--in-diff` run recorded
under "Suggested order" — so the documented gate and the practiced
gate have diverged, and the checklist is executed literally by agents.
Point `check` at `mutants-diff DIFF_BASE=main`, reword AGENTS.md
item 4 to match, keep the full sweep as a scheduled/sharded target
(`--shard k/n` already passes through via `MUTANTS_ARGS`, Makefile:11)
and persist `mutants.out` as the baseline artifact. Explicitly do NOT
add `exclude_globs`: excluding wire-const modules would blind the
sweep to exactly the smoke-killed mutants this project cares about.
1–2 hours, no production code. **Trigger: now.**

### 16. Clock port in `AppServices`

**Landed (2026-07-03).** `app::clock::Clock` (`fn now(&self) ->
SystemTime`) with `adapters::system_clock::{SystemClock, ManualClock}`
(the latter settable/steppable, public so integration smokes can use
it); `SharedClock` on `AppServices`, `RuntimePorts` and
`RuntimeAdapters`; a `.with_clock(...)` knob on the `tests/support`
builder for the N smoke; all 48 production `SystemTime::now()` sites
replaced with `services.clock.now()` (the narrow
`PasswordResetServices` gained a `clock` field). Ratcheted by a new
architecture guard, `app_resolves_now_through_the_clock_port`, which
walks `src/app/` production code and rejects direct
`SystemTime::now()` calls — written first and watched to fail on all
~47 remaining sites. Kill test:
`t_command_renders_the_clock_ports_instant_exactly` pins `T`'s exact
wire bytes under a `ManualClock`. Original problem statement follows.

The domain is already clock-clean (rules take `now: SystemTime`; zero
`SystemTime::now()` hits in domain/), but the app layer resolves "now"
directly at 48 production sites across 14 flow files, so no in-process
test can control the date — the `T` smoke asserts date *shape*, not
value, and the `N` date-scan smoke ("(-X) Days" against the seeded
2026-01..06 corpus) cannot be deterministic. Add a minimal app-layer
`Clock` port (`fn now(&self) -> SystemTime`) as one more `Arc` field
on `AppServices`, a trivial `SystemClock` adapter, a steppable
`TestClock` in test_support plus a clock knob on the `tests/support`
builder (item 12), and mechanically substitute the 48 sites; domain
signatures are unchanged. Upgrade one shape-only time assertion to an
exact pin as the kill test. ~1 day (~20 files; `AppServices` has 17
construction sites across 7 files, mostly the consolidated test
helpers). Also the groundwork for Tier I daily-cap/rollover tests and
item 27's exact-minutes smoke. **Trigger: before the N slice; every
date-stamping slice built first adds migration sites.**

### 17. Extract the NextScan scan engine from `file_list`

The entire NextScan machine — `ScanState`, `run_span` (6 non-self
params, `file_list/mod.rs:174-266`), `stream_dir_body`,
`emit_scan_line`, `scan_more_prompt` (the held-`n`/Q/C/F/R/`?` verb
machine, `:368-485`), the flag repaint/overprint helpers — is private
to the 862-line `file_list/mod.rs` and hard-wired to `F`'s row source.
`N` is pinned to the same door engine (date prompt, then per-dir
`Scanning dir N for <mm-dd-yy>...` headers through the same `More?`
pager). Split the engine into a sibling module — note
`menu_flow/pager.rs` already names the *message* pager, so use e.g.
`file_list/scan.rs` or `menu_flow/nextscan.rs` — generalising
`run_span` so the caller supplies the per-dir row set and header
bytes; `N` then lands as a thin entry point plus its date-prompt wire
consts. Fold in one shared A/U/H/digit span-token resolver: the logic
is currently tripled (`menu_command/files.rs:48-58`,
`file_list/mod.rs:103-116`, `resolve_zippy_span` at `:799-818`), each
caller keeping its own pinned error envelope (the divergent
`Error in input!` vs `No such directory.` wires are legacy parity, not
accidents). 1–1.5 days including moving the pager tests out of the
2,265-line `file_list/tests.rs`. **Trigger: first task of the N
slice — the second consumer keeps the generalisation honest; do not
extract earlier.**

### 18. `FileRepository` port prep: fallibility + file identity

**Landed (2026-07-03).** The three read methods return
`Result<_, FileRepositoryError>` (one opaque
`Backend { source: Box<dyn Error + Send + Sync> }` variant, the item-2
convention); `find_in_area` takes the new
`FileAreaRef { conference, area }` (`domain/files/area.rs`, with
`FileArea::area_ref()`); the error policy — a backend failure logs and
renders exactly like an empty catalogue — lives in one place
(`file_list`'s three read helpers + `empty_on_error`) and is pinned by
an equivalence test written first and watched to fail
(`failing_repository_renders_like_an_empty_catalogue`). The remaining
decisions (write methods with their consuming slices, the
`FileContentStore` split at D-T1, no lock registry until a writer
needs one) are recorded in designs/FILES.md §"Port prep decisions".
One surviving mutant accepted as equivalent: deleting
`empty_on_error`'s `eprintln!` is wire-identical (log-only). Original
problem statement follows.

The port is read-only and infallible by explicit deferral
(`domain/files/repository.rs:7-9` defers `Result` plumbing to D2s),
which bundles a breaking signature change across every call site into
the same slice as a brand-new SQLite adapter — and N and V/VS add more
infallible call sites first. The blast radius is still small: seven
production call sites, all in `file_list/mod.rs`, plus
`bootstrap.rs:354-356`. Prep slice, three decisions: (1) convert the
three read methods to `Result<_, FileRepositoryError>` now, using
item 2's opaque-`Backend` convention; (2) introduce
`FileAreaRef { conference, area }` (mirroring `MessageBaseRef`) as the
port's addressing type — the natural file key
`(conference, area, name)` matches designs/FILES.md's
`UNIQUE(area_id, name)`; (3) record the concurrency decision (no
per-area lock registry until a writer slice needs one) in
designs/FILES.md. Write methods land rule-named with their consuming
slices: `list_new_since` with N, `record_download` with D-T2,
`begin/complete_upload` with D-T4a; a separate `FileContentStore` port
(content bytes vs metadata) lands with D-T1. ~0.5–1 day. **Trigger:
before the N slice — its date query must be a since-bounded port
method, not client-side filtering, because that is the contract D2s
inherits.**

### 19. Re-scope the UTF-8 wire policy to interactive text mode

AGENTS.md declares "the NextExpress wire is valid UTF-8, always",
enforced by `utf8_gate_every_session_byte_decodes`
(`tierd_file_list_smoke.rs:328-350`). A Zmodem payload is arbitrary
bytes — the first transfer slice violates the written contract as-is,
and undirected it will be weakened ad hoc mid-slice. Decide the policy
first: amend AGENTS.md to scope the invariant to **interactive
text-mode traffic**, with the transfer window (between item 20's
raw-channel entry and exit) exempt; the existing gate test drives only
`F` surfaces and survives unchanged. Two adjacent facts: (a) a
pre-existing hole — in `EchoMode::Visible` the codec echoes any
accepted byte ≥ 0x20 raw (`telnet_line.rs:97-109`), so a Latin-1
client typing `©` (lone 0xA9) puts invalid UTF-8 on the wire *today*;
fix alongside item 20 and record the COMMAND_PARITY.md row. (b)
`file_screen_repository.rs:154-157` serves operator screen files as
raw bytes with no validation — a second avenue worth naming in the
policy. Binary test primitives (`read_exact_n` with deadline, raw
write, an IAC escape/unescape helper so expected frames are stated
unescaped, frame-level pins plus before/after-window UTF-8 asserts)
land inside the first slice that exchanges binary bytes — earlier
would be speculation against an undecided seam. Policy ~1 hour; echo
fix ~0.5 day; primitives 0.5–1 day. **Trigger: policy decided before
D-T1 starts.**

### 20. Raw binary channel on the `Terminal` port

The stack cannot carry a Zmodem frame in either direction:
`read_line` lossy-decodes to `String` (`telnet_line.rs:63,83,87`),
`read_key` collapses bytes ≥ 0x80 to `KeyEvent::Other`, `skip_iac`
silently **drops** escaped data 0xFF (IAC IAC —
`telnet_line.rs:167-191`), outbound writes never double 0xFF
(`telnet_listener/mod.rs:160-162` — safe today only because valid
UTF-8 cannot contain 0xFF), and `ColourTerminal::strip_ansi_sgr` would
delete ESC-`[`…`m` byte patterns inside a binary payload while colour
is off. Extend the `Terminal` port (not a separate stream-stealing
transfer port — the D handler reaches the transport only through the
port) with a minimal raw pair: `read_bytes(timeout) ->
RawRead { Bytes(Vec<u8>) | Eof | TimedOut }` — drains the pushback
slot first, no echo by construction, unescapes IAC IAC, and is
**cancellation-safe**: the deadline returns whatever is buffered
rather than dropping already-read payload (the opposite of today's
timeout-wraps-the-future shape) — and `write_raw` (doubles outbound
0xFF); `ColourTerminal` forwards both verbatim regardless of the
colour flag; transfer entry/exit negotiates telnet BINARY (option 0)
in both directions. Default-method trap: a default `write_raw`
delegating to `write` would silently skip 0xFF-doubling if a transport
forgot to override — use inert defaults the transport MUST override,
gated by a smoke (the `read_key` precedent). 1–3 days, ~5–8 files
(socket-pair codec tests, a pushback-drain listener test, mutants
pass). **Trigger: opening sub-slice of D-T1 — wire-neutral and
technically landable earlier, but dead code until then.**

### 21. Zmodem as a sans-IO engine in `app/` (slice-shaping decision)

D-T1 names `amiexpress/zmodem.e` (3,198 lines of callback-into-serial
E code) as the reference. Porting that *shape* would couple protocol
logic to live sockets — untestable without TCP pairs, hostile to the
per-turn mutants discipline on the largest new code body in Tier D —
and D-T-wire explicitly requires an embedded test client, which would
otherwise mean writing the protocol twice. Shape D-T1 as: a pure,
synchronous Zmodem engine (frame encode/decode, CRC16/32, ZDLE
escaping, send/receive session state; inputs = received byte chunks +
elapsed-time ticks; outputs = bytes-to-emit + file-data requests +
progress/outcome events) in `app/zmodem/` — `adapters/` is the one
placement that cannot work, since the handler in `app/menu_flow`
cannot import adapters — plus a thin async pump (~100–200 lines beside
the D handler) marrying engine outputs to item 20's
`write_raw`/`read_bytes` (hard dependency). The smoke harness embeds
the same engine in the opposite role (client-receiver for D-T-wire,
sender for D-T-wire-up). Parity is at the wire, not the E code's
structure (style rule 5). Cost now: recording the decision; the engine
itself is 1–2 weeks spread across D-T1..D-T5. **Trigger: decided
before D-T1 starts; it shapes that slice rather than preceding it.**

### 22. SQLite schema migrations (`PRAGMA user_version`)

There is no schema-evolution mechanism: both SQLite adapters run
`CREATE TABLE IF NOT EXISTS` on every open
(`sqlite_user_repository.rs:96-161`), which never alters an existing
database, and no `user_version`/migration tooling exists anywhere in
src. D-T2 adds `bytes_downloaded_total` to users; `load_user`'s
named-column SELECT (`:350-361`) then fails at statement-prepare on
any pre-existing users.db — **every login breaks after upgrade**. And
D5-persist just made users.db durability a user-visible feature (flags
survive restarts), so existing databases are now expected to survive
upgrades. Small hand-rolled versioned runner in adapters/ (no new
dependency): read `PRAGMA user_version`, apply numbered migrations in
a transaction, bump; wrap the existing DDL as migration 1 so fresh and
old DBs converge; both `open()` paths run it; the D2s file store
starts versioned from day one. Fold in uniform connection setup
(WAL / synchronous / foreign_keys / busy_timeout — currently
asymmetric between the two adapters). ~0.5–1 day, 3–5 files.
**Trigger: before the first schema-altering slice — currently D-T2.**

### 23. Pre-commit the transfer-accounting domain shape

Two halves. (a) **`AuthenticatedCall` struct — standalone, cheap.**
The authenticated-call triple
`(user, authenticated_at, time_remaining)` is duplicated verbatim
across `SessionPhase::Onboarded` and `::Menu` and Option-scattered
across `::LoggingOff`/`::Ended` (`session/mod.rs:188-211` — the latter
pair encode authenticated-ness by convention via independently
nullable Options), with two hand-rolled 8-variant salvage matches
(`:565-601`, `:603-641`) and an 8-variant `time_remaining` accessor
(`:306-317`); every new per-call field costs ~7 destructure/rebuild
sites (lockout, lifecycle's enter_menu `mem::replace` dance,
`tick_minute`, registration). Fold the triple into
`struct AuthenticatedCall` carried by Onboarded/Menu, with
LoggingOff/Ended carrying a salvage enum (`Unidentified |
Identified(User) | Authenticated(AuthenticatedCall)`); accessors keep
signatures so flows don't change, and a new per-call field becomes a
single-site addition. 0.5–1 day; fits the tidy cadence. **Trigger: at
latest before D-T2 adds the first per-call transfer tally.**
(b) **Design pre-commitments, zero code now** (a note to attach to
slices/cmds-files-transfer.md): byte accounting lands as a NEW
`TransferAccounting` value object on `User` (fields typed as `Bytes`),
not more fields on `UsageAccounting`; per-call transfer state lives as
a `Session` **field** (the `ConferenceActivity`/`FlaggedFiles`
sub-mode precedent), never a ninth `SessionPhase` variant; and the
Begin/Complete rule split is already mandated by specs/files.allium
(`BeginDownload`:265, `CompleteDownload`:284, `BeginUpload`:313,
`CompleteUpload`:368), following the messaging rule-module anatomy.
**Trigger: decided before D-T1; executes inside D-T1..D-T4b (the
schema-growth rule forbids adding the fields earlier).**

### 24. Replace the `has_access` all-rights stub

`User::has_access` grants every `Right` — including `Download`,
`Upload`, `OverrideTimeLimit` and `EditFiles` — to any validated
account (`user/mod.rs:580-586`); the doc comment at `:575-578`
concedes the per-tier mapping is "not yet modelled". Harmless while
every gated surface was a messaging command any validated user may
use; load-bearing the moment a command must REFUSE a validated user —
per the slice plan that is FM (legacy gates on `ACS_EDIT_FILES`,
`express.e:24901`) and US (`ACS_SYSOP_COMMANDS`, `express.e:25660`);
D-T2 deliberately ships baseline download *without* the eligibility
check. Scope it minimally: a per-variant narrowing inside
`has_access` (e.g. `EditFiles`/sysop-command rights require
`is_sysop || access_level >= threshold` per the spec's existing
disjuncts), keeping every currently-exercised right granted and
pinning no-behaviour-change over `Right::all()` for landed commands.
Do NOT build a full level→rights table: express.e has no code-level
tier map — `checkSecurity` resolves each ACS flag from per-deployment
icon tooltypes (`express.e:8455-8497`), and specs/core.allium:131-135
leaves the mapping open — so a faithful table is deployment
configuration and would need a config surface it is too early to
design. ~0.5–1 day. **Trigger: with the first refusing slice — the
D/U eligibility refinement if it lands first, otherwise FM/US; before
Tier G/H multiplies sysop-gated surfaces.**

### 25. Grow `NodePool` into the who's-online presence registry

Nothing can answer "who is logged on": the only cross-session state is
`NodePool`'s `Mutex<Vec<Node>>` where `Node` carries
`{number, status}` (`node_pool.rs:14-62`, `domain/node.rs:28-31`),
production nodes never leave `Connecting`
(`NodeStatus::LoggedOn` has zero production references — the
LoggedOn/LoggingOff arcs are dead vocabulary), everything WHO needs
(handle, conference, activity, quiet_mode) is task-local in the
session task, and command handlers cannot even reach the pool
(`Runtime` owns it at `runtime.rs:30`; it is never threaded into
`AppServices`). Keep it concrete — no trait port: the pool is already
app-layer shared state and tests use the real thing. Give each slot an
`Option<NodePresence>` (`{handle, action, conference, logon_at,
quiet_mode}`) with `publish`/`update`/`snapshot_all`; add
`nodes: Arc<NodePool>` to `AppServices`; publish at enter_menu, update
at join and the `Q` toggle, clear at logoff. Two adjacent fixes ride
along: the domain transition table REJECTS `LoggedOn → Idle`
(`node.rs:86-97`) and `release_node_after` discards release errors
(`telnet_listener/mod.rs:139`) — once nodes really reach LoggedOn, a
write-error abort (which skips finalise via `?`) would strand the
node, so add the carrier-loss arc and make `release` clear presence
and force Idle from any live status. 1–2 days, ~8–12 files.
**Trigger: immediately before Tier E's WHO slice — E1 (page-sysop
comment branch) needs none of it, and building it mid-Tier-D would sit
unconsumed for several slices.**

### 26. Per-session `SessionSignal` channel selected inside the terminal

OLM and the page notification must deliver text into a session parked
at a prompt, but a blocked session is suspended inside
`tokio::time::timeout(socket read)` with no other wake source — there
is no mpsc/broadcast/watch/Notify anywhere in production src, and the
`TcpStream` is exclusively `&mut`-borrowed by `TelnetTerminal`
(`telnet_listener/mod.rs:143-146`), so nothing outside the session
task can write to the socket. Hidden trap in the obvious `select!`
retrofit: `read_telnet_line`'s input buffer is a function-local
(`telnet_line.rs:57`) whose per-byte echo has already been emitted, so
a select arm that cancels and restarts the read future silently
discards the user's half-typed command while its echo remains on
screen. Plan: hoist the line buffer (plus an in-progress-read marker)
from codec locals into `TelnetTerminal` fields beside `pushback`, as a
separately-tested codec refactor FIRST; then `NodePresence` (item 25)
carries an `mpsc::UnboundedSender<SessionSignal>`, the per-connection
task keeps the receiver, and `read_line`/`read_key` become a select
over {socket byte, signal, deadline}; on `Deliver(bytes)` the terminal
writes the notification and resumes the same read with buffer and echo
state intact (what the notification does to the on-screen partial line
is a slice-level wire decision, captured against the reference). Start
with exactly one variant, `Deliver(Vec<u8>)`; Tier G later adds `Kick`
(surfaced as synthetic `Eof` so the existing carrier-loss teardown
runs), and the select deadline is item 27's future precise-expiry
hook. ~3 days. **Trigger: opening move of the first delivery slice
(E2 page notification or E5 OLM, whichever schedules first) — not
needed by WHO/WHD; resist landing it with item 25.**

### 27. Activate the inert time budget

`budget::tick_minute` — the only code that decrements
`time_remaining`, accrues `user.time_used_today`, and fires
`LogoffReason::OutOfTime` (`session/budget.rs:74-98`) — has **zero
production callers**; the only budget fn wired in is
`initialise_daily_budget` (from `on_enter_onboarded`,
`lifecycle.rs:231`). So the menu prompt's "mins. left" is frozen for
the whole call and time expiry is unreachable — a live legacy-parity
gap today, and Tier I's daily accounting would otherwise launch on
data that was never recorded. Fix without a ticker task or channels:
track a last-tick timestamp beside `last_input_at` and, at each
menu-loop iteration, apply `tick_minute` once per whole elapsed
minute; on `TickMinuteOutcome::TimeExpired`, write the expiry notice
and return through the existing `DispatchOutcome::LogoffComplete`
path. Every read is already bounded by the 5-minute input timeout, so
expiry fires at worst one idle-timeout late (time spent inside
sub-flows accrues retroactively on return) — acceptable until Tier G,
when precise mid-read expiry can ride item 26's select deadline. The
expiry notice is a NEW wire surface: FS-UAE capture + type-at-it check
required, and the `ACS_OVERRIDE_TIMELIMIT` behaviour (`express.e:557`
— override holders never expire) needs a decision. 1–2 days.
**Trigger: opportunistic during Tier E, ideally after item 16 so the
smoke can pin exact minutes; hard deadline before Tier G's time-adjust
slice (G6).**

## Refactorings to skip for now

- **Splitting the crate into workspace members.** Module boundaries and
  the architecture test already give us the invariants a workspace
  split would enforce. The split would add ceremony before the domain
  is stable.
- **A DI framework.** `AppServices` plus plain `Arc<dyn …>` is already
  the simplest thing that works.
- **Generic-everywhere `AppServices`.** Parameterising on
  `<U: UserRepository, H: PasswordHasher, …>` would buy compile-time
  specialisation at the cost of code-size blow-up across every flow
  signature, and would block runtime adapter swapping (which is the
  whole point of holding ports behind `Arc<dyn …>`). Type erasure here
  is intentional.
- **Async-fn-in-trait for `ScreenRepository`.** Until `RPITIT` works
  cleanly behind `Arc<dyn Trait>`, the manual `Pin<Box<dyn Future>>`
  alias is the shortest path. Revisit when `dyn` support catches up.
- **Standalone RPITIT conversion of `Terminal`.** There are 11
  `impl Terminal` sites, **9 of them `#[cfg(test)]` doubles** (~29
  `Box::pin` wrappers in test code vs ~6 in production), so the win is
  in writing the consolidated capture-terminal double once without
  `Box::pin` — fold it into refactoring 12, which rewrites those exact
  impls, not as a standalone change. Verify `Send` still holds at the
  `tokio::spawn` boundary (it should, since the spawn resolves at the
  concrete `ColourTerminal<TelnetTerminal>`). Keep `ScreenRepository`
  and `MailStores` on the manual alias — they are genuinely `Arc<dyn>`.
- **A dyn `Command`-trait registry for menu dispatch.** Handlers are
  inherent async methods on `MenuFlow<'a, T: Terminal>`; a dyn
  registry would force type-erasing the terminal (rippling through
  `ColourTerminal` and every flow) for no line savings. The dispatch
  `match` arms also encode real heterogeneous behaviour — `G`'s
  early return, `J`'s NoAccess→logoff, `CF`'s rights gate — not
  ceremony. Checked and rejected by the June 2026 assessment; the
  data-driven shape that *does* fit is the parser-side listing
  (refactoring 11).
- **Rewriting the legacy wire strings.** The legacy strings *are* the
  wire contract and stay verbatim: nothing about their content is a
  refactoring target. (Refactoring 9 — co-locating each command's text
  with its command, now landed — moved the strings to where they are
  emitted without changing any of them; `app/wire_text.rs` is left at 36
  lines of cross-cutting primitives. That was a placement policy, not a
  rewrite.)
- **Moving the flag sub-mode (the `A` loop, `G` confirm + autosave,
  logon restore) out of `menu_flow/mod.rs` into a sibling module.**
  Checked by the July 2026 review: a pure ~2–4 h code move with no
  roadmap blocker. Do it opportunistically when next in the file
  (item 13's rule), not as a scheduled slice.

## Suggested order

Refactorings 3, 4, 5, 6, 7, 8 and 9 have **landed** (June 2026), one
commit each, with the full suite plus a focused `cargo mutants
--in-diff` run per commit, and the late-June `tidy:` commits landed
item 12's crate-side half. The July 2026 forward-looking review
re-sequenced what remains around the roadmap's tier order (SLICES.md):

0. **Correctness bugs surfaced by the review:**
   - **Fixed (2026-06-23):** the `EH` edit-header abort bug.
     `read_optional_unchanged_line` conflated "blank = keep current"
     with "EOF/idle = abort", so an idle timeout silently kept the field
     and committed the edit instead of aborting it. The reader now
     returns the three-state `Option<Option<String>>` it always
     documented and `handle_edit_header` returns early on abort, **silent**
     per the legacy `editHeader` (`express.e:11602`). Pinned by three
     unit tests in `sysop_admin.rs`. The reader merge (refactoring 10)
     that would encode the same distinction is still open.
   - **Fixed (June 2026):** the save → `.expect()` panic on persistence
     failure, across all three sign-in/​logoff save points:
     - `LoginFlow::authenticate` (`verify_password`) — the
       `Save(UserRepositoryError)` arm now logs and returns the new
       `LoginOutcome::Aborted`, which the driver turns into a clean
       connection close.
     - `SessionDriver::enter_menu` (`enter_menu`) — the `Save` arm
       returns `Err`, and the driver logs + closes the connection
       rather than admitting the caller with unsaved logon state.
     - `SessionDriver::finalise` (`finalise_logoff`) — the `Save` arm is
       logged and swallowed (the session is already closing).

     In each case only the genuinely-impossible typestate arm panics:
     `verify_password`/`finalise` use `unreachable!` (the wrappers
     guarantee the state); `enter_menu`'s `Session` arm is the
     not-yet-wired `force_password_reset` path and keeps a descriptive
     `panic!` until the password-reset slice handles it. Each landed as
     its own slice with a failing test first.
0b. **Open defects surfaced by the July 2026 review:**
   - **Fixed (2026-07-02): flag-identity data loss (item 14).** The
     same file could occupy two `FlaggedKey`s (real area vs the
     `A`-prompt/restore `area=0`); the `A` listing duplicated the name
     in any config, and under `user_storage = sqlite` the logoff flag
     save hit a PK violation and silently lost the session's flag
     changes. `FlaggedKey` no longer carries an area.
   - **Inert time budget (item 27):** `tick_minute` has no production
     caller — the menu prompt's "mins. left" is frozen and
     `OutOfTime` logoff is unreachable (legacy-parity gap).
   - **Latin-1 echo hole (item 19):** `EchoMode::Visible` echoes any
     accepted byte ≥ 0x20 raw, so a Latin-1 client typing `©` puts
     invalid UTF-8 on the wire today, against the AGENTS.md invariant.

1. **Now, before FS:** item 14 (the live flag-identity defect),
   item 15 (mutation-gate re-scope, 1–2 h), and item 12's
   smoke-harness half — before the FS smoke becomes helper-copy #7.
2. **Before N:** item 16 (clock port), then as part of the N slice
   itself: item 17 (scan-engine extraction, first task of the slice)
   and item 18's `list_new_since` (the port-prep slice — Result
   plumbing + `FileAreaRef` — lands just before). Item 10's menu_flow
   reader merge before N/FM, per its own trigger.
3. **Between the read-only slices and D/DS:** item 2 (the
   error-boundary pass, before D2s copies the leaky template), item 1
   (command-style user writes, before D-T2's ledger deltas — the big
   one at 4–6 days), item 22 (schema migrations, hard requirement
   before D-T2), item 23a (`AuthenticatedCall`).
4. **With D-T1:** item 19's policy decision + echo-hole fix first,
   item 20 (raw binary channel) as the opening sub-slice, item 21
   (sans-IO Zmodem engine) as the slice's shape. Item 23b executes
   inside D-T1..D-T4b.
5. **With the first refusing slice** (the D/U eligibility refinement
   or FM/US): item 24 (`has_access` narrowing).
6. **Tier E:** item 25 (presence registry) immediately before WHO;
   item 26 (`SessionSignal` channel) as the opening move of the first
   delivery slice (E2/E5); item 27 (time-budget activation)
   opportunistically after item 16, hard deadline before Tier G's G6.
   Item 12's two-session smoke primitives stage with the first Tier E
   slice.

Still opportunistic, unscheduled: the declarative command listing (11)
when next in `menu_command.rs`; the remaining file-size moves (13) —
`session_flow.rs` per-rule split, the `sqlite_user_repository.rs` /
`file_screen_repository.rs` test promotions — one-per-commit when each
file is quiet; the `Terminal` RPITIT conversion bundled into whichever
slice next rewrites the terminal doubles.
