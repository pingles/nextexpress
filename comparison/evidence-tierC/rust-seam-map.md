# Rust seam map — Tier C conference navigation (C2, C3, C4a, C4b)

Evidence gathered from the current tree at commit `1992800` (branch `main`).
All paths relative to `/Users/paul/Documents/GitHub/nextexpress` unless noted.

---

## 1. Command parsing — `rust/src/app/menu_command.rs`

### Shape today

- `MenuCommand` enum: `rust/src/app/menu_command.rs:9-67`. Variants today:
  `Logoff` (G), `Join(NumberArg)` (J), `Read(NumberArg)` (R), `ScanAllMail` (MS),
  `Post(PostArg)` (E), `CommentToSysop` (C), `ShowTime` (T), `ShowVersion` (VER),
  `ShowHelp` (H), `QuietToggle` (Q), `ShowStats` (S), `ExpertToggle` (X),
  `ShowMenu` (?), `TopicHelp(String)` (^), `AnsiToggle` (M),
  `ConferenceFlags` (CF), `Unknown`.
- The parser is deliberately effect-free (`menu_command.rs:1-5`); all I/O lives
  in `MenuFlow`.
- `parse_menu_command(line: &str) -> MenuCommand` (`menu_command.rs:93-147`):
  1. `line.trim()`.
  2. A ladder of whole-line `eq_ignore_ascii_case` checks for the no-argument
     commands (G, C, CF, T, VER, H, Q, S, X), exact `==` for `?` (line 122),
     `strip_prefix('^')` for topic help (line 125), `M` (line 128).
  3. `parse_number_command(trimmed, "J")` then `..., "R")` (lines 131-136).
  4. Whole-line `MS` (line 140), then `parse_post_command` for E (line 143).
  5. Fallthrough → `MenuCommand::Unknown` (line 146).

### `NumberArg` (`menu_command.rs:70-79`)

```rust
pub(crate) enum NumberArg { Number(u32), Missing, Invalid }
```

`parse_number_command` (`menu_command.rs:149-165`) is **whitespace-split**:
`line.split_ascii_whitespace()`; head token must equal the command
(case-insensitive); no second token → `Missing`; a third token → `Invalid`;
second token parsed with `str::parse::<u32>()`, failure → `Invalid`.
So `J` → `Join(Missing)`, `J 7` → `Join(Number(7))`, `J nope` / `J 1 2` →
`Join(Invalid)`.

### Multi-char symbol tokens (`<`, `>`, `<<`, `>>`)

There is no generic tokenizer — each command match inspects the trimmed line
itself, so symbol commands tokenize cleanly as whole-line equality checks
exactly like `?` does (`menu_command.rs:122`). `trimmed == "<<"` and
`trimmed == "<"` cannot collide as long as `<<` is *either* checked before `<`
or (better) both are exact whole-line `==` comparisons — `"<<".trim() == "<"`
is false, so ordering is actually irrelevant for exact matches. Nothing else
in the ladder consumes `<` or `>` (the only prefix-style match is
`strip_prefix('^')`). `JM` / `JM <n>` slots straight into
`parse_number_command(trimmed, "JM")`; it must be checked **before**
`parse_number_command(trimmed, "J")` is irrelevant too because
`parse_number_command` matches the *whole head token* (`head.eq_ignore_ascii_case("J")`
rejects `JM`), so order does not matter — but placing `JM` next to `J` reads
best.

### Where new variants go

- New enum variants in `MenuCommand` with `///` doc comments carrying
  `amiexpress/express.e:NNNN` provenance (house style — see every existing
  variant).
- Parse arms before the `Unknown` fallthrough.
- In-module tests in `mod tests` (`menu_command.rs:187+`).

### The menu-advertisement trip-wire (will fail on any new variant)

`main_menu_advertises_exactly_the_implemented_commands`
(`menu_command.rs:526-559`) reads `Conf02/Menu5.txt` (repo root), takes the
first whitespace token of each indented line, keeps tokens the parser
recognises, and asserts that set equals the set derived from
`advertised_token` (`menu_command.rs:569-589`, an **exhaustive match** — a new
variant fails to *compile* until given a token) applied to
`every_menu_command()` (`menu_command.rs:594-614`, must also gain a sample).
Therefore every new command (C2 keeps `J`; C3 adds `<`, `>`; C4a adds `JM`;
C4b adds `<<`, `>>`) requires:
1. `advertised_token` arm, 2. `every_menu_command()` sample,
3. an indented entry in `Conf02/Menu5.txt` (current CONFERENCES section is at
   `Conf02/Menu5.txt` lines ~22-24: `J <n>` and `CF`).

---

## 2. The join flow today

### Dispatch — `rust/src/app/menu_flow/mod.rs`

- `MenuFlow<'a, T: Terminal>` struct: `mod.rs:59-65`; fields `terminal: &'a mut T`,
  `services: &'a AppServices`. Per-command handlers live in sibling files as
  `impl<'a, T: Terminal> MenuFlow<'a, T>` blocks (`mod.rs:10-13`).
- Menu loop `run(&mut self, mut session: MenuSession) -> Result<LoggingOffSession, T::Error>`
  (`mod.rs:78-121`): optional menu screen (non-expert), `format_menu_prompt`,
  `read_prompted(prompt, TerminalEcho::Visible)`, `parse_menu_command`,
  `dispatch`.
- `dispatch` (`mod.rs:165-294`) returns `DispatchOutcome::Continue(MenuSession)`
  or `LogoffComplete(LoggingOffSession)` (`mod.rs:53-56`). The Join arm
  (`mod.rs:187-198`):

```rust
MenuCommand::Join(arg) => match arg {
    NumberArg::Number(n) => { ... handle_explicit_join(session, n) ... }
    NumberArg::Missing => self.write_and_flush(JOIN_REQUIRES_NUMBER_LINE).await?,   // mod.rs:196  ← C2 replaces this
    NumberArg::Invalid => self.write_and_flush(INVALID_CONFERENCE_NUMBER_LINE).await?, // mod.rs:197
},
```

Note the Join arm is the only one that *consumes* the session by value
(because `explicit_join_conference` is a typed transition consuming
`MenuSession`); all other handlers take `&mut session`.

### Handler — `rust/src/app/menu_flow/join.rs`

- `ExplicitJoinResult` enum (`join.rs:24-29`): `Joined(MenuSession)` /
  `NoAccess(LoggingOffSession)`.
- `pub(super) async fn handle_explicit_join(&mut self, session: MenuSession,
  target_conference_number: u32) -> Result<ExplicitJoinResult, T::Error>`
  (`join.rs:35-78`). Sequence on `Joined`:
  1. `format_explicit_join_line(conferences, conference_number, msgbase_number)`
     computed up-front (borrow discipline, `join.rs:54-58`),
  2. if `!matched_request` → `NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE`
     (`join.rs:59-62`),
  3. write join line, `render_name_type_promotion` (`session_presenter.rs:116-132`),
  4. `scan_mail_on_join(&mut session)` (`join.rs:90-122` — locks current base via
     `super::lock_current_base`, runs `scan_mail` from the read pointer,
     renders `SCREEN_MAILSCAN` + `render_scan_summary`).
  On `NoAccess`: `NO_CONFERENCE_ACCESS_LINE` then `LoggingOffSession`.
- **C2 reuse**: after the interactive prompt resolves a number, call
  `handle_explicit_join` unchanged. **C3** can also funnel into it once the
  prev/next target number is computed. **C4a/C4b caveat**: it always lands on
  the conference's *primary* msgbase (see §3) — there is no msgbase-targeted
  join today.

### Domain transitions

- `MenuSession::explicit_join_conference(self, target, &[Conference], now)
  -> ExplicitJoinTransition` — `rust/src/domain/session/typed.rs:362-393`;
  outcome enum `ExplicitJoinTransition` at `typed.rs:520-546` (fields:
  `session`, `conference_number`, `msgbase_number`, `show_bulletin`,
  `matched_request`, `name_type_promoted_to`).
- Underlying `Session::explicit_join_conference` —
  `rust/src/domain/session/conferencing.rs:141-179`: resolves, then
  `user.record_join(conference, msgbase)` (updates `User.last_joined`),
  name-type promotion, `activity.attach(...)` (closes prior visit, opens new
  one — the `SessionsHaveAtMostOneOpenVisit` invariant).
- Resolution logic — `rust/src/domain/conference_visit.rs`:
  - `resolve_explicit_join(target, user, conferences) -> JoinResolution`
    (`conference_visit.rs:270-296`): direct hit when granted, else falls
    through to `first_accessible_conference` with `matched_request = false`,
    else `NoAccess`.
  - `resolve_auto_rejoin` (`conference_visit.rs:235-253`): `last_joined` if
    still granted, else first accessible.
  - `primary_msgbase_of(conference)` (`conference_visit.rs:214-225`): base
    number 1, else first declared. **Every join path resolves the msgbase via
    this helper** — JM needs a new resolution path.
  - `next_accessible_conference_after(user, conferences, after_number)`
    (`conference_visit.rs:189-198`): lowest-numbered granted conference with
    `number > after_number`. **Directly reusable for `>`**; a `prev_…`
    mirror is the missing piece for `<`.
  - `JoinReason` enum exists (`conference_visit.rs:20-30`:
    `AutoRejoin` / `ExplicitJoin` / `ConfScanWalk`) but is currently decorative
    — visits do not record it.
- `Session::auto_rejoin_conference` (`conferencing.rs:69-105`), driven from the
  driver at `rust/src/app/session_driver.rs:131-183` (announcement deferred
  until after the logon conference scan; `announce_auto_rejoin` at
  `session_driver.rs:197-214`).
- `MenuSession` accessors C3/C4 need: `current_conference_number()`
  (`typed.rs:276-280`), `current_msgbase() -> Option<(u32, u32)>`
  (`typed.rs:286-289`), `user()` / `user_mut()` (`typed.rs:252-268`),
  `attach_read_visit(conf, base, now)` (`typed.rs:314-322` — re-points the
  open visit **without** `last_joined` bookkeeping / promotion / scan; built
  for the MS read-it-now detour, legacy `express.e:11750-11758`).

### Current-base resolution helpers (refactoring 8, commit `3a7da7d`)

Free functions at the bottom of `rust/src/app/menu_flow/mod.rs`, consumed by
every command module via `super::`:

- `current_base(session: &MenuSession) -> Option<MessageBaseRef>` — `mod.rs:352-357`.
- `lock_current_base(session, mail_stores) -> Option<(MessageBaseRef, MailStoreGuard)>`
  — `mod.rs:362-372` (async; `None` when no open visit or no registered store).
- `allowed_addressing_for(conferences, base) -> Option<AllowedAddressing>` —
  `mod.rs:376-381`.

C4a/C4b's "list this conference's msgbases / join one" should follow the same
pattern: resolve the conference from `session.current_conference_number()`
against `self.services.conferences.as_ref()` (e.g. via
`Conference::find_msgbase`, `conference.rs:305-307`).

---

## 3. Conference & message-base domain model

`rust/src/domain/conference.rs`:

- `Conference` (`conference.rs:220-315`): `number: u32` (1-indexed),
  `name: String`, `msgbases: Vec<MessageBase>` (non-empty, enforced by
  `Conference::new`/`with_name_type`, `conference.rs:244-280`),
  `accepted_name_type: NameType`. Accessors: `number()`, `name()`,
  `msgbases()` (declared order), `find_msgbase(msgbase_number)`
  (`conference.rs:305`), `accepted_name_type()`.
- `MessageBase` (`conference.rs:115-207`): **yes, a distinct concept** —
  `conference_number`, `number` (1-indexed within conference), `name`,
  `allowed_addressing`, `all_scan_scope`; `msgbase_ref()` → `MessageBaseRef`.
- `MessageBaseRef` (`conference.rs:77-107`): `(conference_number, msgbase_number)`
  coordinate pair, `Copy`, `Ord`.
- `ConferenceMembership` (`conference.rs:338-501`): per-(user, conference) —
  `conference_number`, `granted: bool` (revoked rows kept for history),
  `pointers: Vec<ReadPointers>` (per-msgbase, lazily upserted),
  `messages_posted`, and the four `CF` scan flags (`ScanFlag` enum,
  `conference.rs:356-369`). Membership is **per conference, not per msgbase**
  — there is no msgbase-level access gate.
- Free helpers: `first_accessible_conference(memberships, conferences)`
  (`conference.rs:513-520`), `has_membership(memberships, conference)`
  (`conference.rs:526-530`), `find_msgbase_in(catalogue, coord)`
  (`conference.rs:538-543`).

Enumeration & access:

- The catalogue is `AppServices.conferences: SharedConferences =
  Arc<Vec<Conference>>` (`rust/src/app/services.rs:34,58`), loaded once at
  startup; the `ConferenceRepository::load_all` contract guarantees ascending
  `number` order (`rust/src/domain/conference_repository.rs:66-71`; file
  adapter `rust/src/adapters/file_conference_repository.rs` reads
  `Conf<NN>/conference.toml`). Numbers need not be contiguous — prev/next
  must walk the sorted slice, not do `n ± 1` arithmetic.
- Access checks on the user: `User::has_membership(&Conference)`
  (`rust/src/domain/user/mod.rs:699`), `has_granted_membership_for(u32)`
  (`user/mod.rs:716`), `memberships()` (`user/mod.rs:662`),
  `last_joined() -> Option<MessageBaseRef>` (`user/mod.rs:728`),
  `record_join(&Conference, &MessageBase)` (`user/mod.rs:734`).
- Rights gate example for command-level ACS: `Right` enum
  (`user/mod.rs:61-86`, e.g. `Right::EditConferenceFlags` used by `CF` at
  `menu_flow/mod.rs:285-289`). No join-specific right exists today.

---

## 4. Interactive sub-prompt precedent

### Primitives

- Port: `Terminal` trait — `rust/src/app/terminal.rs:43-75`
  (`write`, `flush`, `read_line(echo, timeout)`, `ansi_colour`,
  `set_ansi_colour`). `TerminalEcho::{Visible, Masked}` (`terminal.rs:19-24`);
  `TerminalRead::{Line(String), Eof, IdleTimedOut}` (`terminal.rs:28-35`).
- Free helper `read_prompted(terminal, prompt, echo, timeout)`
  (`terminal.rs:83-95`): write prompt → flush → `read_line`.
- `MenuFlow::read_prompted(&mut self, prompt, echo)` wrapper —
  `menu_flow/mod.rs:296-303`; pulls the timeout from
  `self.services.session_policy.input_timeout()`.
  `MenuFlow::write_and_flush` — `mod.rs:305-307`.

### The canonical "prompt, read a line, blank aborts" loops

1. **CF flags editor** — `rust/src/app/menu_flow/conf_flags.rs:29-74`. The
   exact idiom C2 should copy:

```rust
let TerminalRead::Line(mask_line) = self
    .read_prompted(CONF_FLAGS_MASK_PROMPT, TerminalEcho::Visible)
    .await?
else {
    return Ok(());                       // Eof / IdleTimedOut → back to menu
};
session.record_input(SystemTime::now()); // stamp the idle clock
let Some(flag) = parse_scan_flag_mask(&mask_line) else {
    return Ok(());                       // blank / invalid → back to menu
};
```

   The menu loop's *next* read then applies carrier-loss / idle-timeout —
   sub-prompts never apply those transitions themselves (see comment at
   `read_subprompt.rs:89-91`).

2. **R read sub-prompt** — `rust/src/app/menu_flow/read_subprompt.rs:39-148`
   (`run_read_subprompt`): a `loop` that re-renders the prompt each turn,
   same `let TerminalRead::Line(..) = … else { return Ok(()) }` exit, and
   `session.record_input` after each accepted line.

3. **Shared one-shot helpers** (`pub(super)`, usable from any menu_flow
   submodule) — `rust/src/app/menu_flow/post_mail.rs`:
   - `read_required_line(&mut self, session, prompt, silent) -> Result<Option<String>>`
     (`post_mail.rs:411-432`): trims; **empty / Eof / Idle → `None`**;
     `silent = true` suppresses the `Message aborted.` notice (`silent =
     false` writes `POST_ABORTED_LINE` — mail-specific, so C2 wants
     `silent = true` or the CF-style inline idiom).
   - `read_optional_line` (`post_mail.rs:446-461`): returns `Some("")` for a
     blank line (private; used by the `To:` → ALL reroute).

C2's flow is therefore: write `joinconf_screen()` bytes → CF-style prompt read
→ blank/non-numeric aborts to menu → number feeds `handle_explicit_join`.

---

## 5. Screens

- Port: `ScreenRepository` — `rust/src/app/screens.rs:13-131`. **A JOINCONF
  hook already exists and is documented but unused in production**:
  `fn joinconf_screen(&self) -> ScreenFuture<'_>` (`screens.rs:36-53`), doc
  comment: `SCREEN_JOINCONF` (`Screens/JoinConf.txt`,
  `amiexpress/express.e:6588-6590`), "Rendered as the prompt header when the
  user typed `J` without a conference number … (`amiexpress/express.e:25143`)".
  The same comment records that `SCREEN_JOIN` / `SCREEN_JOINED` are
  deliberately *not* on this port (they are new-user-registration screens,
  `express.e:30057/:30125`; the join wire lines are inline in legacy
  `joinConf`, `express.e:5071-5085`).
- Adapter: `rust/src/adapters/file_screen_repository.rs` —
  `joinconf_bytes()` at `file_screen_repository.rs:245-249` reads
  `<bbs_path>/Screens/JoinConf.txt` through `cached_file`
  (`file_screen_repository.rs:121-142`: cached after first read,
  `normalise_to_crlf` translation of Amiga line endings).
  **When the file is absent it falls back to a built-in, non-empty header**:
  `FALLBACK_JOINCONF = b"\r\nJoin which conference?\r\n"`
  (`file_screen_repository.rs:32-41`) — unlike `bbs_help`/`logoff` whose
  fallback is empty bytes with caller-side handling. Trait impl at
  `file_screen_repository.rs:339-341`; adapter tests at
  `:647-656` (loads from disk) and `:700-703` (fallback).
- Only call sites today are adapter tests and a test-stub in
  `rust/src/app/session_driver.rs:386`. C2 wires the first production call:
  `let screen = self.services.screens.as_ref().joinconf_screen().await;`
  then `self.terminal.write(&screen)` before the prompt — because the
  fallback is non-empty, no absent-file branch is needed (write
  unconditionally).
- Pattern references for other Tier C screens: `mailscan_screen` call in
  `join.rs:111-113`; conference menu resolution `render_menu_screen`
  (`menu_flow/mod.rs:314-332`) → `conference_menu(conf, access_level)` /
  `default_menu(access_level)` with the legacy security-level walk
  (`file_screen_repository.rs:164-231`).

---

## 6. Wire text conventions — `rust/src/app/wire_text.rs`

- One module of `pub(crate) const NAME: &[u8] = b"...";` byte constants plus
  `pub(crate) fn render_*(...) -> Vec<u8>` builders (1,703 lines). Every item
  carries a doc comment with legacy provenance in the form
  `` `amiexpress/express.e:NNNN` `` and, where applicable, the note that the
  Amiga `\b\n` becomes telnet `\r\n` (e.g. `HELP_UNAVAILABLE_LINE`,
  `wire_text.rs:122-127`).
- CRLF conventions: notices are usually `\r\n`-prefixed *and* terminated
  (`b"\r\nInvalid conference number.\r\n"`); prompts end with `": "` or
  `" >: "` and **no trailing CRLF** (e.g. `CONF_FLAGS_MASK_PROMPT`,
  `wire_text.rs:1052-1053`; `POST_TO_PROMPT`, `:282`). ANSI SGR escapes are
  baked into the literals (`EDITOR_MSG_OPTIONS_PROMPT`, `:314-315`).
- Tier-C-relevant items:
  - `JOIN_REQUIRES_NUMBER_LINE = b"\r\nUsage: J <conference-number>\r\n"` —
    `wire_text.rs:243`; its doc comment (`:239-242`) explicitly marks it as
    the simplified Phase-4 stand-in "future slices may refine this when the
    `JoinConf` prompt arrives". **C2 deletes this constant** (it is a
    NextExpress invention, no legacy provenance).
  - `INVALID_CONFERENCE_NUMBER_LINE` — `wire_text.rs:247` (also reused by the
    MV move target, `sysop_admin.rs:241`; deleting it is not an option,
    re-purposing is).
  - `NO_ACCESS_TO_REQUESTED_CONFERENCE_LINE` — `wire_text.rs:236-237`
    (legacy `express.e:25157`).
  - `NO_CONFERENCE_ACCESS_LINE` — `wire_text.rs:230`.
  - `auto_rejoin_line` — `wire_text.rs:991-1008`
    (`Conference <n>: <name> [<mb>] Auto-ReJoined`, legacy `:5071-5073`).
  - `explicit_join_line` — `wire_text.rs:1023-1034`
    (`\x1b[32mJoining Conference\x1b[33m:\x1b[0m <name> [<mb>]`, legacy
    `:5079-5083`). Both append ` [<msgbase>]` only for multi-base conferences
    — the `Some(_)`/`None` decision is made by
    `resolve_conference_strings` (`rust/src/app/session_presenter.rs:22-40`,
    mirroring legacy `getConfMsgBaseCount(conf)>1`).
  - Presenter wrappers: `format_auto_rejoin_line` (`session_presenter.rs:49-57`),
    `format_explicit_join_line` (`:63-71`), `format_menu_prompt` (`:89-112`,
    prompt label is `name` or `name - msgbase`, legacy `:28413-28421`).

---

## 7. Tests

### Where tests live

- **Unit tests are in-module** under `#[cfg(test)] mod tests` at the bottom of
  each file: parser tests in `menu_command.rs:187-615`; handler tests in
  `menu_flow/mod.rs:383-505`, `menu_flow/join.rs:125-333`,
  `read_subprompt.rs:282-417`; domain tests in `conference.rs:569-896`,
  `conference_visit.rs:298-573`.
- Handler-test fixtures: a local `CaptureTerminal` (write-capturing,
  `read_line` → `Eof`; e.g. `join.rs:150-176`) plus a hand-rolled
  `test_services()` building `AppServices` as a struct literal
  (`join.rs:187-224`); `MenuFlow` is constructed by struct literal
  `MenuFlow { terminal: &mut terminal, services: &services }`
  (`join.rs:271-274`). For *scripted input* (which C2's prompt tests need)
  the precedent is `FakeTerminal` in `session_driver.rs:307-361` — a
  `VecDeque<TerminalRead>` of inputs, captured output, recorded echo modes.
- Integration smokes are top-level files in `rust/tests/`. Mutation testing
  config: `rust/.cargo/mutants.toml` (cargo-mutants via nextest; see
  AGENTS.md workflow).

### The in-process telnet smoke shape (canonical: `rust/tests/quickwins_smoke.rs`)

Per AGENTS.md rule 6, new command-family smokes copy this harness:

- `spawn_listener_at_bbs_path(bbs_path) -> SocketAddr`
  (`quickwins_smoke.rs:455-498`): builds in-memory adapters
  (`InMemoryUserRepository` seeded with `seed::default_sysop` +
  `seed::grant_all_memberships`, `Pbkdf2PasswordHasher`, `InMemoryCallerLog`,
  `InMemoryMailStores`), one `Conference::new(1, "Main", [MessageBase 1])`,
  a `Config { max_nodes: 1, max_password_failures: 3, bbs_path, ..default }`,
  `bootstrap::build_runtime(...)`, `TelnetListener::bind("127.0.0.1:0", runtime)`,
  `tokio::spawn(listener.run())`, returns the bound addr.
  `spawn_listener_with_seeded_sysop()` (`:447-449`) roots at cwd.
- `sign_in_seeded_sysop(&addr) -> TcpStream` (`:503-513`): drives
  `ANSI Graphics (Y/n)? ` → `Y` → `Enter your Name: ` → `sysop` →
  `PassWord: ` → `sysop` → drains to the stable menu-prompt tail
  `b"mins. left): "`.
- `write_line` (`:546-550`, body + CRLF), `drain_until(stream, needle)`
  (`:556-579`, 2 s `DRAIN_DEADLINE`, panics with the captured transcript),
  `contains` / `find` (`:581-587`), `end_session` (`:518-521`, `G` →
  `Goodbye`).
- Multi-conference smoke precedent (J navigation across Conf01-03 with
  per-conference `menu.txt` markers and a real-names conference):
  `rust/tests/phase4_smoke.rs:60-216` — but note that one spawns the binary;
  new Tier C smokes should stay in-process per AGENTS.md.

### Tests pinning the CURRENT non-legacy `Usage: J <conference-number>` behaviour

Greps for `Usage: J` / `JOIN_REQUIRES` across `rust/src` and `rust/tests`:
**no test pins the usage line itself** — the only references are the constant
(`wire_text.rs:243`) and its single call site (`menu_flow/mod.rs:196`); stale
hits exist only in `rust/mutants.out.old/` logs. So the bare-`J` dispatch
branch is currently a known mutation-test blind spot, and C2 replaces it
without breaking an existing wire assertion. What *does* pin today's J
behaviour and needs attention in C2-C4b:

| Test | Location | Impact |
| --- | --- | --- |
| `parses_join_command_arguments` | `rust/src/app/menu_command.rs:198-219` | Pins `"J"` → `Join(Missing)` (still true in C2 — only the handler changes), `"J nope"` → `Invalid`, `"J 1 2"` → `Invalid`. Revisit if the legacy no-arg prompt accepts non-numeric input differently. |
| `main_menu_advertises_exactly_the_implemented_commands` + `advertised_token` + `every_menu_command` | `rust/src/app/menu_command.rs:526-614` | Fails to compile / asserts on every new variant; `Conf02/Menu5.txt` must gain `<`, `>`, `<<`, `>>`, `JM` entries. |
| `binary_walks_phase4_conference_flow_over_telnet` | `rust/tests/phase4_smoke.rs:30` (J 2 at `:155`, J 3 at `:179`, J 99 at `:198`) | Pins `J <n>` happy path, real-names promotion, `do not have access` fallback — unaffected unless C-slices change `J <n>` wire output. |
| phase6 smoke | `rust/tests/phase6_smoke.rs:206` (`J 1`) | Same — `J <n>` only. |

(No in-module dispatch test exercises `MenuCommand::Join(NumberArg::Missing)`
today.)

---

## 8. Echo / terminal — what an interactive prompt must respect

- **Echo policy** is per-read: `Terminal::read_line(echo, timeout)`. Menu and
  every existing sub-prompt use `TerminalEcho::Visible`; only password prompts
  use `Masked`. The telnet adapter (`rust/src/adapters/telnet_line.rs:51-134`)
  performs **server-side echo** (the listener advertises `IAC WILL ECHO`):
  visible echoes the byte, masked echoes `*` (`telnet_line.rs:123-127`);
  BS/DEL erase with `<BS><SPACE><BS>` (`:107-114`); CR / CRLF / CR-NUL / bare
  LF all terminate a line, with a one-byte pushback so SyncTerm bare-CR
  clients don't double-Enter (`:92-106`, `:156-169`); IAC negotiation is
  stripped inline (`:66-91`); control bytes < 0x20 are ignored; lines are
  capped at `MAX_TERMINAL_LINE_BYTES` (`rust/src/app/input_limits.rs`).
  Submitting a line always emits `\r\n` to the client, so the server's
  response text conventionally *also* starts with `\r\n` for a blank
  separator line (see the `*_LINE` constants).
- **Timeout**: always pass `self.services.session_policy.input_timeout()` —
  which `MenuFlow::read_prompted` (`menu_flow/mod.rs:296-303`) does for you.
  Handle all three `TerminalRead` variants; sub-prompts treat `Eof` /
  `IdleTimedOut` as "leave the sub-flow" and let the menu loop's next read
  fire the carrier-loss / idle-timeout transitions
  (`menu_flow/mod.rs:101-114`, `read_subprompt.rs:89-96`).
- **Idle clock**: call `session.record_input(SystemTime::now())` after every
  accepted `Line` (every existing prompt does; `typed.rs:83-94`).
- **Colour**: `ColourTerminal` decorator (`rust/src/app/colour_terminal.rs:58-103`)
  wraps the transport at the composition root; while the `M` toggle is off it
  strips ANSI SGR runs from **every write** (`strip_ansi_sgr`,
  `colour_terminal.rs:32-52`). New prompts/screens may therefore embed ANSI
  colour freely (matching legacy literals) — they degrade automatically.
  Don't pin raw ANSI bytes in a smoke without accounting for the toggle state
  (default is colour ON; `terminal.rs:66-69`).

---

## Gaps / design seams the C slices must add (not present today)

1. **C2**: an interactive join prompt flow — `joinconf_screen()` write + a
   CF-style prompt read + numeric parse, feeding `handle_explicit_join`.
   Deletes `JOIN_REQUIRES_NUMBER_LINE`. New prompt constant belongs in
   `wire_text.rs` with `express.e:25143`-area provenance.
2. **C3**: a `prev_accessible_conference_before` mirror of
   `next_accessible_conference_after` (`conference_visit.rs:189`), plus
   parser variants for `<` / `>`. Decide (from legacy evidence) what happens
   at the ends of the catalogue — current `explicit_join` fallback semantics
   (first-accessible + no-access notice) are probably *not* what legacy `<`/`>`
   do.
3. **C4a/C4b**: msgbase-targeted join. Every existing join resolves
   `primary_msgbase_of`; `MenuSession::attach_read_visit` re-points the visit
   but skips `User.last_joined` bookkeeping. A proper `JM` needs either a new
   typed transition (e.g. `explicit_join_msgbase`) that runs
   `user.record_join(conference, msgbase)` + `activity.attach` for a
   non-primary base, or an extension of `explicit_join_conference` taking an
   optional msgbase. Read pointers are already per-msgbase
   (`ConferenceMembership.pointers`, `conference.rs:341/465-500`), so no
   storage change is needed.
