# SLICES.md

Incremental delivery plan for the Rust port of AmiExpress (NextExpress).

The plan is **organised around the AmiExpress menu commands**. Each
remaining slice ships one user-typeable command (or a tight cluster of
sub-commands that share the same code path) end-to-end: parser →
domain rule → adapter → wire bytes → smoke test. A slice is done when
the legacy user could type the command, see the verbatim AmiExpress
wire text, and the rule and its invariants are backed by tests. Slices
are ordered so the most user-visible commands land first and so each
slice introduces only the seams the *next* slice will need.

Each slice is sized to fit a 15–20 minute TDD session: write a failing
test, write the minimum code to pass, mutate to verify the test
catches real bugs, then refactor.

Spec references use the form `<file>:<RuleOrEntity>` and point at
[the Allium specs](specs/) as the source of truth. Legacy E source in
`amiexpress/` is referenced only for original strings and to disambiguate
fine details. The canonical legacy menu dispatch table lives at
`amiexpress/express.e:28285` (`processInternalCommand`).

This file is the small, always-loaded index. Per-slice **In Scope** /
**Out of Scope** detail lives in [`slices/`](slices/), one file per
command family — load only the family you're working on.

## Foundation (already shipped)

The Cargo crate, the telnet listener, the session state machine, the
user / conference / mail entities, the on-disk repositories, and the
legacy-format menu prompt (`<bbsName> [<conf>:<name>] Menu (<n> mins.
left): `, Tier A A4) are in place. The commands shipped so far are:
`G`, `J`, `R`, `E`, `C`, `RP`, `FW`, `K`, `MV`, `EH`, `T`, `VER`, `H`,
`Q`, plus the whole of Tier A's quick wins — `S`, `X`, `?`, `^`, `M`
(and `MS`) — the auto-mail-scan and auto-rejoin hooks fired by `J`,
and Tier D's `F` (the NextScan file lister, slices D1+D2).
The canonical record of what each shipped slice covers is the
[Allium specs](specs/) plus the code and its tests; the per-slice
"In Scope" history is in git.

`M` was rebound to its legacy ANSI-toggle meaning in Tier A (A8); the
scan-all it used to carry now lives on `MS`. `N`'s mail-scan binding
(a NextExpress drift) was removed in Tier B (B2) — `N` is now an
unknown command until Tier D ships the new-files scan (the
board-as-shipped AquaScan date-scan experience; the internal scan at
`express.e:25275` is door-shadowed — see
[cmds-files-list.md](slices/cmds-files-list.md)).

## Login-sequence fixes

Parity fixes to the logged-on bring-up sequence (between a successful
password and the first menu prompt) — foundation logon behaviour, not
menu commands. Detail in [`slices/login-fixes.md`](slices/login-fixes.md).

| Fix | Legacy source | Slice file | Status |
| --- | --- | --- | :---: |
| Logon conference scan (multi-conference new-mail scan) | `express.e:28066` (`confScan`) | [login-fixes.md](slices/login-fixes.md) | Done (L1) — the logon walk reuses the `MS` render + read-it-now flow, filtered to `mail_scan`-flagged bases, run before the auto-rejoin |

## Menu-command roadmap

Each remaining slice maps to one legacy menu command (or a tight
cluster). The table is ordered by user-visible value: small, common,
no-dependency commands land first; commands that need new subsystems
(file transfer, OLM, …) come once the prerequisite slice has been
done.

| Cmd | Legacy source | Slice file | Tier | Status |
| :---: | --- | --- | :---: | :---: |
| **A. Quick wins (small commands, no new subsystems)** ||||
| `?` | `express.e:24594` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `T` | `express.e:25622` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `S` | `express.e:25540` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `VER` | `express.e:25688` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `H` | `express.e:25071` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `X` | `express.e:26113` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `M` | `express.e:25239` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done (mis-binding fixed; scan-all → `MS`) |
| `Q` | `express.e:25504` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `^` | `express.e:25089` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Done |
| `S` extended report | `express.e:25540` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Todo (A11, after Tier I) |
| `NS` non-stop pagination | `express.e:24627, 24644, 26170` | [cmds-quickwins.md](slices/cmds-quickwins.md) | A | Todo (A12 — runtime `ns` keystroke + pause suppression already done in `pager.rs`; the `NS` argument-token plumbing and a paginated consumer remain) |
| **B. Mail UI completion** ||||
| `MS` | `express.e:25250` | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (folded B3's listing table in) |
| `N` (mail) | NextExpress-only drift — no legacy `N`→mail (legacy `N` = new-files, `express.e:25275`, see Tier D) | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (`N` → unknown; new-files scan deferred to Tier D) |
| `R` sub-prompt | `express.e:11972` (`readMSG`) | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (B4–B6: the `A`/`F`/`R`/`D`/`M`/`EH`/`L`/`?`/`??`/`<CR>`/`Q` loop, gated, 20 telnet smokes) |
| `R` no-arg entry + legacy `readMSG` loop | `express.e:11984, 12008-12230` | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (B10 — prompt-first bare `R` at the read-pointer; next-to-read range + `( QUIT )` exhausted prompt; reshaped the shared loop, so `R <num>` matches too) |
| Scan listing rows | `express.e:11713-11739` | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (B3, shipped with B1's `MS`) |
| Retire top-level mail shortcuts | (cleanup) | [cmds-mail-finish.md](slices/cmds-mail-finish.md) | B | Done (B8 — `RP`/`FW`/`K`/`MV`/`EH` now parse to unknown) |
| **C. Conference navigation** ||||
| `<` / `>` | `express.e:24529, 24548` | [cmds-conf-nav.md](slices/cmds-conf-nav.md) | C | Done (C3 — nearest accessible neighbour via the sorted-catalogue walk, joined through the same machinery as `J <n>` (byte-identical output); past either edge the command opens the C2 interactive prompt, no wraparound; the legacy `ACS_JOIN_CONFERENCE` gate stays unported for consistency with the ungated `J`) |
| `JM <n>` | `express.e:25185` | [cmds-conf-nav.md](slices/cmds-conf-nav.md) | C | Done (C4a — in-range `JM <n>` re-runs the full join sequence on the current conference (multi-base announcements append ` [<base>]`, no "already there" check); single-base conferences fail with the verbatim "does not contain multiple message bases" notice; a `.`-dotted first token delegates the raw params to `J`, whose dotted / two-token forms now join the requested base; missing/out-of-range bases open C4b's `Message Base Number (1-N): ` prompt, deviations recorded in the slice doc) |
| `<<` / `>>`, `JM` interactive | `express.e:24566, 24580, 25197` | [cmds-conf-nav.md](slices/cmds-conf-nav.md) | C | Done (C4b — `<<` / `>>` step the current conference's bases through the full join machinery, falling into the `JM` no-arg flow past either edge (single-base notice, or the `Message Base Number (1-N): ` prompt on multi-base conferences); the single-shot prompt renders the conference-local-first `JoinMsgBase` screen, blank aborts; `JM`'s prompt answer clamps into `[1,N]` while `J`'s passes unclamped to the domain's reset-to-primary — the legacy asymmetry, pinned by tests) |
| `J` no-arg prompt | `express.e:25113-25183` | [cmds-conf-nav.md](slices/cmds-conf-nav.md) | C | Done (C2 — bare / non-numeric / out-of-range `J` opens the single-shot `Conference Number (1-N): ` prompt (`Val` + clamp, blank aborts); denied joins keep the caller in their current conference with the legacy notice instead of falling through; dotted / two-token msgbase forms joined the requested base when C4a landed, the base request surviving the conference prompt) |
| `CF` | `express.e:24672` | [cmds-conf-nav.md](slices/cmds-conf-nav.md) | C | Done (C5 — landed first in Tier C; M/A/F/Z editor, flags on `ConferenceMembership` (SQLite-persisted), `*` honours the advertised toggle-all the legacy no-ops) |
| **D. Files — browsing first, transfer second** ||||
| `F` (file listings) | AquaScan door (shadows `express.e:24877` — see [evidence-tierD](comparison/evidence-tierD/live-observations.md)) | [cmds-files-list.md](slices/cmds-files-list.md) | D | Done (D1+D2 — the NextScan lister over the seeded in-memory catalogue, byte-pinned to the live captures with the three NextScan branding swaps; pager verbs incl. the in-pager help; six-scenario telnet smoke. True single-key hotkey pager landed in slice D2b (probe-pinned held-`n`/Enter and bare-LF corners); UTF-8 wire = slice D2u; F/R flag into a session set with an on-row `[X]` marker + in-place repaint = slice D2f. Still Todo: the SQLite file-metadata store (slice D2s, deferred until the first file-writer slice) and the FlagFile/UnflagFile rule layer + downstream flag surfaces — the logon `** Flagged File(s) Exist **` banner, clean-logoff warning and autosave banner (slice D5)) |
| `FR` (reverse listings) | AquaScan door (shadows `express.e:24883`) | [cmds-files-list.md](slices/cmds-files-list.md) | D | Done (D3 — the `FR` token reuses the D2 lister with `reverse=true`: banner `'fr ?'` (dash run flexed 40→39), `Reverse-scanning dir N... Ok!` header, files newest-first; bare `FR` opens the `Directories:` prompt under the reverse banner (like bare `F`), then reverse-walks the chosen span — following `express.e:27645` over the AquaScan capture, which skips the prompt for `FR`; `FR A` descends highest→lowest per `express.e:27654`; `F R` with a space stays the `F`-with-junk Argument-error path) |
| `Z` (zippy search) | `express.e:26123` | [cmds-files-list.md](slices/cmds-files-list.md) | D | Todo |
| `A` list flagged set | `express.e:24601` | [cmds-files-list.md](slices/cmds-files-list.md) | D | Todo (D6a) |
| `A` add/remove flagged | `express.e:24604` | [cmds-files-list.md](slices/cmds-files-list.md) | D | Todo (D6b) |
| `FS` (file status) | `express.e:24872` | [cmds-files-list.md](slices/cmds-files-list.md) | D | Todo |
| `N` (new files scan) | AquaScan door (shadows `express.e:25275`) | [cmds-files-list.md](slices/cmds-files-list.md) | D | Todo (after the mail-`N` semantic fix) |
| `D` / `DS` (download) | `express.e:24853` | [cmds-files-transfer.md](slices/cmds-files-transfer.md) | D | Todo |
| `U` (upload, baseline) | `express.e:25646` | [cmds-files-transfer.md](slices/cmds-files-transfer.md) | D | Todo (D-T4a) |
| `U` upload accounting refinements | `express.e:25646` | [cmds-files-transfer.md](slices/cmds-files-transfer.md) | D | Todo (D-T4b) |
| `RZ` (instant upload) | `express.e:25608` | [cmds-files-transfer.md](slices/cmds-files-transfer.md) | D | Todo |
| `V` / `VS` (view file) | `express.e:25675` | [cmds-files-transfer.md](slices/cmds-files-transfer.md) | D | Todo |
| `FM` (file maintenance) | `express.e:24889` | [cmds-files-sysop.md](slices/cmds-files-sysop.md) | D | Todo |
| `US` (sysop upload) | `express.e:25660` | [cmds-files-sysop.md](slices/cmds-files-sysop.md) | D | Todo |
| **E. Communication with other users / sysop** ||||
| `O` (page sysop) | `express.e:25372` | [cmds-comm.md](slices/cmds-comm.md) | E | Todo |
| `OLM` (node-to-node) | `express.e:25406` | [cmds-comm.md](slices/cmds-comm.md) | E | Todo |
| `WHO` | `express.e:26094` | [cmds-comm.md](slices/cmds-comm.md) | E | Todo |
| `WHD` | `express.e:26104` | [cmds-comm.md](slices/cmds-comm.md) | E | Todo |
| **F. User self-service** ||||
| `W` (change user info) | `express.e:25712` | [cmds-user-self.md](slices/cmds-user-self.md) | F | Todo |
| `B <n>` (direct bulletin) | `express.e:24648` | [cmds-user-self.md](slices/cmds-user-self.md) | F | Todo (F5a) |
| `B` interactive bulletin index | `express.e:24634` | [cmds-user-self.md](slices/cmds-user-self.md) | F | Todo (F5b) |
| `GR` (greets) | `express.e:24411` | [cmds-user-self.md](slices/cmds-user-self.md) | F | Todo |
| **G. Sysop session control + in-session F-keys** ||||
| F1 sysop direct logon | (key combo) | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G1) |
| Local logon + `RL` relogon | `express.e:25534` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G2) |
| Node reservation | spec rule `session.allium:ReserveNodeForUser` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G3) |
| Node suspend / resume / shutdown | spec rules in `session.allium` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G4) |
| Sysop kick | spec rule `session.allium:SysopKick` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G5) |
| Sysop in-session time +/- (F2/F3) | `express.e:7864-7876` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G6) |
| Sysop display-file + capture (Shift-F4 / F4) | `express.e:7878-7889` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G7) |
| Sysop temp-access grant (Shift-F6) | `express.e:7899-7921` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G8) |
| Sysop available toggle (F7) | `express.e:7923-7930` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G9) |
| Sysop "switch user" UX | (wraps `RelogonRequested`) | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G2b) |
| Page reserved-for-X user | spec rule `session.allium:ReserveNodeForUser` | [cmds-sysop-session.md](slices/cmds-sysop-session.md) | G | Todo (G3b) |
| **H. Sysop console (`F6`-class) commands** ||||
| `1` account read-only / search | `express.e:24453` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo (H1a) |
| `1` account field edits | `express.e:24453` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo (H1b) |
| `2` (caller log viewer) | `express.e:24461` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `3` (edit dir file) | `express.e:24511` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `4` (edit any file) | `express.e:24517` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `5` (dir anywhere) | `express.e:24523` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `0` (remote shell) | `express.e:24424` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `NM` (node monitor) | `express.e:25281` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `UP` (node uptime) | `express.e:25667` | [cmds-sysop-console.md](slices/cmds-sysop-console.md) | H | Todo |
| `CM` (conference maint.) | `express.e:24843` | — | H | Skipped (see below) |
| **I. Accounting + crypto refinements** ||||
| Per-conference accounting | spec — `core.allium:ConferenceMembership` | [cmds-accounting.md](slices/cmds-accounting.md) | I | Todo |
| Credit accounts | spec — `core.allium:CreditAccount` | [cmds-accounting.md](slices/cmds-accounting.md) | I | Todo |
| Daily byte cap end-to-end | spec — `files.allium:DailyDownloadsLeQuota` | [cmds-accounting.md](slices/cmds-accounting.md) | I | Todo |
| Low-credit ratio weighting (`lcfiles`) | spec — `files.allium:File.status` | [cmds-accounting.md](slices/cmds-accounting.md) | I | Todo (I3b) |
| Legacy + lower-round password hashes | spec — `core.allium:PasswordHashKind` | [cmds-accounting.md](slices/cmds-accounting.md) | I | Todo |
| **J. Lower-priority / niche commands** ||||
| `ZOOM` (QWK gather) | `express.e:26215` | [cmds-misc.md](slices/cmds-misc.md) | J | Todo |
| `VO` (voting booth) | `express.e:25700` | [cmds-misc.md](slices/cmds-misc.md) | J | Todo |
| `WALL` / external command dispatcher | `express.e:28258` (`runSysCommand`) | [cmds-external.md](slices/cmds-external.md) | J | Todo |
| Per-conference external command overrides | `express.e:28258` | [cmds-external.md](slices/cmds-external.md) | J | Todo (X3) |
| **Future / not yet sliced** ||||
| SSH, FTP, HTTPd, QWK, FTN, IEMSI, axSetupTool replacement | — | [future.md](slices/future.md) | — | Future |
| Xmodem / Ymodem / Hydra alt transport | `amiexpress/xpr*.e` | [future.md](slices/future.md) | — | Future |
| OS-level signal handling (daemon stop) | — | [future.md](slices/future.md) | — | Future |
| Sysop bulk file import (CLI wizard) | — | [future.md](slices/future.md) | — | Future |
| Browse-side lha smoke harness | — | [future.md](slices/future.md) | — | Future |

Each command-family file lays out its slices in the order the table
suggests, and is the only file you need to load when working on
commands in that tier. A command-family file owns its own
**wire-and-smoke closing slice**: a phase whose theme names a
user-visible capability is only **Done** once that capability is
reachable by running the compiled binary, not merely the library
tests — same rule as the foundation phases.

## Concurrency model

The BBS is one process serving many concurrent sessions. We use tokio
for the async runtime: the listener is async, each accepted connection
runs in its own tokio task, and shared stores sit behind async-aware
locks (`tokio::sync::Mutex` / `RwLock`, or `dashmap` where appropriate).

The `Node` entity is the unit of concurrency. At most
`core/config.max_nodes` sessions run at once; the supervisor enforces
this with a `Semaphore` and the `OneActiveSessionPerNode` invariant
(`session.allium`). Sessions don't share state beyond what the spec
models — the user record, the message base, the file area — so
contention is fine-grained: one lock per message-base or area, never a
global one. `messaging.allium`'s `lock_msgbase` predicate is one such
lock; `User` mutations from a single session are serialised by virtue
of one task per session.

Wire protocols are pluggable. Telnet is the first transport (Slice 8);
SSH and FTP are listed under future phases and will plug into the same
per-task accept-loop pattern. From the supervisor's point of view a
transport is just an `AsyncRead + AsyncWrite` byte stream.

Async-friendliness is therefore part of the design from Slice 1 (which
brings in tokio) onwards, not something we retrofit later.

## How slices grow the schema

Each slice introduces only the data shape — entity fields, enum variants,
config keys, value types — that its rules actually read or write. Later
slices extend the shape as their rules need more. We don't pre-create a
"complete" `User`, `Session`, `Config` or `PasswordHashKind` up front and
fill it in over time; we add the field, the variant or the config key in
the slice that first consumes it.

Concretely:

- `User` starts as the bare set of fields needed to look someone up and
  verify their password; `account_locked`, `force_password_reset`,
  `is_new_user`, time accounting, byte tallies, ratio mode and conference
  memberships each arrive with the slice that introduces the rule
  reading them.
- `Session` starts with what `AcceptConnection` and the state machine
  need; the boolean presentation flags (`ansi_colour`, `quick_logon`,
  `rip_mode`, `quiet_mode`, `cmd_shortcuts`, `expert_mode`) land in the
  toggles slice that exposes them (Tier A — `cmds-quickwins.md`).
- `Node.status` starts as the subset Phase 1 transitions through;
  `reserved`, `suspended` and `shutting_down` land with their commands
  in `cmds-sysop-session.md`.
- `PasswordHashKind` starts with one variant (`pbkdf2_10000`, the spec's
  default for new accounts); the legacy 32-bit hash and the lower-round
  pbkdf2 levels arrive when an older user record forces us to read them
  (Tier I — `cmds-accounting.md`).
- `Config` is grown a key at a time, each key landing in the slice whose
  rule reads it (e.g. `max_password_failures` lands with the
  password-failure slice, `input_timeout` with the idle-timeout slice).

## Adapter contracts

The Allium specs in `specs/` deliberately exclude wire-level concerns
(see `session.allium`'s "Excludes: Wire transports (telnet IAC, FTP
control connection, serial CD) — these are surface concerns; session
sees only `remote` or `local`"). That's correct hexagonal modelling
— the domain shouldn't know whether bytes flow over telnet, SSH, FTP
or serial — but it leaves a class of obligations on every user-facing
transport adapter that no Allium rule will ever describe. Those
obligations are written down here so each adapter slice owns them
explicitly instead of inheriting silent expectations.

### Wire-quality checklist for user-facing transport adapters

Any slice that introduces or extends a user-facing transport adapter
(today: telnet — Slice 8 / Slice 8a; future: SSH, FTP, web, …) must
satisfy this checklist before it can be marked **Done**. Each item is
testable; the slice owns failing tests and adapter code that makes
them pass.

1. **Input echo, visibility-aware.** Every typed printable byte is
   echoed back to the client. The default mode is *visible* (echo
   the literal byte). At password-class prompts the mode is *masked*
   (echo `*` instead) — the password must never appear on the wire.
   Mirrors `amiexpress/express.e:2342` (`aePuts(cmdCharString)` in
   `lineInput`) and `amiexpress/express.e:1543` (`serPuts('*')` in
   `getPass2`).
2. **Line editing.** `<BS>` (`0x08`) and `<DEL>` (`0x7F`) remove the
   previous byte from the input buffer and emit `<BS><SPACE><BS>` to
   the client to erase the position visually. A backspace at an empty
   buffer is a no-op (no underflow, no spurious echo). Mirrors
   `amiexpress/express.e:1530-1538` (`getPass2`) and `:2304-2320`
   (`lineInput`).
3. **CRLF discipline on every server-originated byte stream.** All
   server output uses telnet `\r\n`, not bare `\n` and not Amiga
   `\b\n`. Disk-loaded screen files authored on the original Amiga
   (which used `\b\n` as end-of-line) are translated to `\r\n` on the
   way out. Pressing `<Enter>` echoes a CRLF so the cursor advances
   on the client.
4. **Control-byte filtering.** Bytes below `0x20`, other than the
   four explicitly handled ones (`<CR>`, `<LF>`, `<BS>`, `<DEL>`),
   are silently dropped from accepted input. Mirrors `lineInput`'s
   `IF (ch>31)` guard at `amiexpress/express.e:2335`.
5. **Protocol-level negotiation handled, not echoed.** Any `IAC`
   sequences (telnet option negotiation, subnegotiation) are consumed
   by the adapter and never appear in the input buffer fed to the
   domain. The set of negotiations the adapter advertises (e.g.
   `WILL ECHO`, `WILL SUPPRESS-GO-AHEAD`) creates obligations the
   adapter must then fulfil.
6. **End-to-end byte assertion in the phase smoke test.** The
   wire-and-smoke closing slice for any phase that exposes new
   user-facing prompts must read the bytes the client actually
   receives between writes — not just look for the next prompt — and
   assert that visible echo / mask invariants hold there. A test
   that only checks "the next prompt appeared" passes against a
   server that ignores everything until `\r\n` and never echoes.

### Original strings: parity with the AmiExpress source

Where the original BBS already has a user-facing string — a prompt,
an error message, a banner, a status line, a command character — we
use it verbatim. We don't invent new wording when a legacy original
exists, even when the legacy wording is awkward, ungrammatical, or
slightly inconsistent with itself. Parity with what the existing
sysop and user community already know is the goal; reflowing the
prose costs that parity for nothing in return.

Concretely, each slice that introduces a user-facing string must:

- **Find the original first.** Grep the `amiexpress/` tree (typically
  `express.e`, sometimes `axenums.e`, `axconsts.e`, or an asset under
  `deployment/`) for the prompt, message or command. AGENTS.md's
  rule applies: "Always use the `amiexpress` source when referencing
  original strings/messages/commands etc."
- **Carry the source line as a comment** next to the constant or
  string literal, of the form
  `// amiexpress/express.e:NNNN`.
  This makes the lineage auditable and lets future readers verify
  parity at a glance.
- **Translate Amiga line endings only.** The legacy `\b\n` becomes
  telnet `\r\n`; legacy `[<n>m` ANSI escapes pass through unchanged;
  the textual content is preserved character-for-character (modulo
  obvious mojibake of `©` / `é` / similar from the original file's
  encoding, which we restore via `\u{...}` escapes). The wire is
  valid UTF-8 always — encoding policy is owned by the "Wire
  encoding" section in AGENTS.md; that section supersedes any
  earlier slice-level guidance about emitting raw Latin-1 bytes.
- **Document any deliberate departure** in the slice's In Scope, with
  reasoning. "We renamed X to Y because Z" belongs in the slice spec
  so it isn't quietly drift.

If an asset (`Menu.txt`, `BBSTITLE.txt`, screen file) ships in
`amiexpress/deployment/binaries.lha` (see the asset inventory at the
foot of this file), the adapter loads that asset rather than
rendering a built-in fallback. The fallback exists only for the
"sysop hasn't dropped the file in place yet" case and is built to
look as close to the legacy default as we can make it.

### Parity is at the wire boundary, not the line boundary

Carrying the legacy strings forward is non-negotiable; carrying the
legacy *implementation shape* forward is not. The `amiexpress/` tree
is Amiga `E`, which has none of `std`'s combinators, no `time` crate
format descriptions, no iterator chaining — so a literal line-for-line
port often produces awkward Rust (hand-rolled two-digit padders,
manual byte buffers built one push at a time, etc.). Prefer the
idiomatic Rust expression of the same behaviour: `format!`,
`time::macros::format_description!`, iterator chains, `Display` impls.
The tests pin the wire output verbatim, so the parity surface is
preserved; the implementation underneath is the Rust author's
business.

## Done means done

A slice is **Done** only when every Allium rule, invariant and black-box
function listed in its "In Scope" section is implemented, backed by tests
that pass, and `cargo test`, `cargo build`, `cargo fmt --check` and
`cargo clippy -- -D warnings` are all clean. Anything else is **Todo**
(or **In progress** while a slice is being worked on).

A **command-family file** whose theme names a user-facing capability
is **Done** only once each of its commands is reachable by running the
compiled binary — not merely the library or per-test in-process
listeners. Every such file therefore owns a closing slice that wires
the composition root (`app::main`), pins down the runtime config
acquisition story and the seed-data story, and adds a smoke test that
spawns the binary process and exercises the headline flow end-to-end.
Library-level slice tests are necessary but not sufficient: a
command-family whose binary wouldn't actually deliver its commands is
**In progress**, not Done.

## Skipped slices

Slices land here when the work has been considered and deliberately not
done, rather than merely deferred. A skipped slice keeps its identity
(so other slices that reference it don't quietly resurrect it) but
ships no rules, no code and no tests. The entry below explains why —
so a future contributor doesn't quietly bring the slice back without
revisiting the reasoning.

### `CS` (conference scan) command — original Slice C1

There is **no `CS` command** in AmiExpress. The legacy dispatch table
(`processInternalCommand`, `express.e:28285`) has no `CS` token, and the
live FS-UAE reference confirmed it (2026-06-03). The runtime
multi-conference mail scan is `MS` (`internalCommandMS`, already
shipped); the conference scan modelled by `conferences.allium:ConferenceScan`
is the *logon-time* `confScan()` (`express.e:28066`), not a menu command.
The original C1 entry proposed a `CS` command with an invented
`Conference <n>:` / `<CR>=next, S=stop` UX — dropped as drift. The
per-conference scan flags `confScan()` consults are edited by `CF`
(Slice C5) and gate the conference mail-scan and the `N` new-files scan
(Tier D); no `CS` command is planned.

### Conferences (admin) — original Phase 5 (Slices 35, 36) and `CM`

The legacy `CM` command (`express.e:24843`, `conferenceMaintenance()`)
and the original Slices 35–36 (`SysopCreatesConference`,
`SysopGrantsConferenceAccess`, `SysopRevokesConferenceAccess`) all
overlap. On re-reading the legacy source none of them corresponds to a
runtime command that survives the move to file-based config, and none
of them has a planned caller in any other slice:

- **No legacy runtime "create conference".** `cmds.numConf` is read
  once at startup from the `NCONFS` tooltype
  (`amiexpress/express.e:31791`). `conferenceMaintenance()`
  (`amiexpress/express.e:22686`) only edits *existing* conferences
  (ratios, mail-scan pointer resets, capacity). The legacy "create a
  conference" workflow is stop-BBS / edit-tooltypes /
  make-directories / restart — which already maps onto NextExpress's
  "drop a `Conf<NN>/conference.toml` and restart" (Slice 28's
  `FileConferenceRepository`).
- **No legacy runtime "grant/revoke access" command pair.** Access is
  held in the user record's 10-char `conferenceAccess` field and is
  only mutated by the F6 account editor's `F` field
  (`express.e:21446`), the `PRESET.AREA` tooltype copier
  (`:21333`) or the Shift-F6 temporary-access swap (`:7900`).
- **No planned caller anywhere.** Tier F's `W` command edits the
  user's *own* info only. Tier G (sysop session control) is entirely
  about local logon and node-level operations. The legacy F6 account
  editor and `conferenceMaintenance()` are subsumed by file edits
  (and the possible CLI wizard in [`slices/future.md`](slices/future.md))
  rather than in-BBS commands, per `AGENTS.md`'s "configuration via
  files rather than a separate program" rule.

If a future in-BBS sysop-admin surface lands, the rules can be revived
then — they will have a caller and a user-visible behaviour to anchor
the tests.

### Per-slice deferrals that are deliberately not picked up

Some "Out of Scope" bullets in the command-family files refer to
behaviour the legacy supports but that NextExpress will not
implement. They are listed here so a future contributor can see the
deferral was deliberate.

| Origin slice | Deferral | Why not |
| --- | --- | --- |
| A6 (`X` expert mode) | Per-conference menu expert variants | Legacy supports per-conference variant strings; NextExpress uses one expert-mode boolean. Adds complexity without changing the user-observable behaviour for the common case. |
| A8 (`M` ANSI toggle) | Per-screen RIP mode rendering | `Session.rip_mode` is recorded by `AcceptConnection`, but no command surface needs it and no slice ships an RIP-aware renderer. RIP terminals are vanishingly rare in 2026. |
| C5 (`CF` flags editor) | Forced-newscan / no-newscan tooltype overrides | The legacy stored these as `.info` tooltypes. Per `AGENTS.md` we use file-based config; the sysop can edit `Conf<n>/conference.toml` directly. |
| D6b (`A` add/remove) | Cross-conference flagging | Legacy permits flagging from outside the current conference in some configurations. Adds a flag-disambiguation UX without a clear win — defer permanently unless asked. |
| E5 (`OLM`) | File attachments to OLMs | Legacy `OLM` is single-line; attachments belong to mail (`AttachFileToMail`) and there's no parity story for adding them here. |
| F3 (`W` extended) | Handle changes (sysop-only) | Not in the spec schema; the closest legacy surface is the F6 account editor's name field, which `cmds-sysop-console.md`'s H1b covers. |
| F5b (`B`) | Per-security bulletin variants beyond `findSecurityScreen` | The on-disk fallback chain already lets a sysop ship per-tier bulletins by file naming; an additional config layer is duplication. |
| G1 (sysop direct logon) | `instantLogon` sysop key combo | Tracked as an open question in `session.allium`. If the spec ever closes the question the slice can be added; until then there's no rule to implement. |
| G9 (sysop available toggle) | Auto-availability based on idle time | Manual toggle matches legacy behaviour and avoids surprising the sysop when a long compile or reading session flips them to "away." |
| H5 (`NM` actions) | Real-time stats overlays | Legacy `NM` doesn't have them; not a planned feature. |
| I2 (credit accounts) | Payment / billing integration | Credit accounts are tracked; the means of *funding* them is deliberately external (cash, donation, etc.). |

### Amiga-only sysop console keys (F8 / F9 / Shift-F10)

These three sysop console keys are deliberately not sliced because the
hardware / OS facility they wrap doesn't exist on the platforms
NextExpress targets:

- **F8 — SER-OUT toggle** and **F9 — SER-IN toggle**
  (`amiexpress/express.e:7932-7942`). The legacy emits / consumes
  bytes via the Amiga serial port hardware. Telnet (and the planned
  SSH transport) have no equivalent — there is nothing to toggle.
- **Shift-F10 — clear tooltype cache** (`amiexpress/express.e:7957-7960`).
  The legacy caches resolved `.info` tooltype lookups for performance
  on slow disks. NextExpress reads TOML on each start; there is no
  cache to clear.

If a future SSH or serial adapter ever brings the underlying
facility back, these can be revived then.

## Asset inventory (from `amiexpress/deployment/binaries.lha`)

The lha was inspected and the following assets are usable as seeds. Note
that `defaultbbs/Screens/` ships empty — the named SCREEN_* files
(BBSTITLE, AWAIT, LOGON, LOGOFF, NEWUSERPW, JOIN, JOINED, MAILSCAN, etc.,
as enumerated in `amiexpress/axenums.e:19`) were authored per sysop and
are not bundled. Slices that need a screen will either use a built-in
default we author, or use a file the sysop drops on disk at the
configured path.

| Asset | Use |
| --- | --- |
| `defaultbbs/Conf02/Menu.txt` | Default ANSI conference menu (2.4 KB, full command set). Used by Slice 12 as the menu shown after logon. |
| `defaultbbs/Conf01/menu.txt` | Minimal "Lamer Land" 4-command menu (G/O/C/U). Useful as a low-access-tier menu fixture. |
| `defaultbbs/Conf01/path`, `paths`, `NDirs` | Tiny on-disk format for "where does this conference live" — reference for Slice 28 conference loader. |
| `defaultbbs/Conf01/MsgBase/MailStats`, `MailLock` | Seed files showing the message-base on-disk schema; reference for Slice 37 mail store. |
| `defaultbbs/Conf01/Conf.DB`, `defaultbbs/Conf02/Conf.DB` | Empty conference databases; layout reference for Slice 28. |
| `defaultbbs/user.data`, `user.keys`, `user.misc` | Three-file user schema (legacy split). Reference for Slice 3 user repository; the port may collapse to one file. |
| `defaultbbs/SystemStats` | Binary stats template; reference only. |
| `defaultbbs/Documentation/Aedoc4.guide` | Original AmigaGuide manual — search here for any user-facing string we need to mirror exactly (prompts, error wording). |
| `defaultbbs/Access/*.info`, `defaultbbs/Commands/BBSCmd/*.info`, `defaultbbs/FCheck/*.info`, `defaultbbs/Protocols/Xpr*.info` | Amiga tooltype configs. Reference only — per `AGENTS.md`, the Rust port stores config in files, not icon tooltypes. |
| `amiexpress/express.e:6539` (`displayScreen`) | Authoritative list of which SCREEN_* names the BBS dispatches and the order they appear in. |
| `amiexpress/express.e:28285` (`processInternalCommand`) | Authoritative legacy dispatch table — the canonical list of `internalCommandX` procs and the tokens that reach them. |
