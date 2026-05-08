# SLICES.md

Incremental delivery plan for the Rust port of AmiExpress (NextExpress).

Each slice is sized to fit a 15–20 minute TDD session: write a failing
test, write the minimum code to pass, mutate to verify the test catches
real bugs, then refactor. Slices are ordered so user-visible value lands
as early as possible, and so that later slices can build on the seams
introduced by earlier ones without retro-fitting.

Spec references use the form `<file>:<RuleOrEntity>` and point at
[the Allium specs](specs/) as the source of truth. Legacy E source in
`amiexpress/` is referenced only for original strings and to disambiguate
fine details.

This file is the small, always-loaded index. Per-slice **In Scope** /
**Out of Scope** detail lives in [`slices/`](slices/), one file per phase
— load only the phase you're working on.

## Phases

| Phase | File | Slices | Theme | Status |
| --- | --- | --- | --- | :---: |
| 0 | [slices/phase0.md](slices/phase0.md) | 1 | Project foundations | Done |
| 1 | [slices/phase1.md](slices/phase1.md) | 2–13, 8a, 13a | Sign in, see the menu, log off | Done |
| 2 | [slices/phase2.md](slices/phase2.md) | 14–18 | Hardening the logon flow | Done |
| 3 | [slices/phase3.md](slices/phase3.md) | 19–21 | New user onboarding | Done |
| 4 | [slices/phase4.md](slices/phase4.md) | 22–26 | Sysop console & node controls | Todo |
| 5 | [slices/phase5.md](slices/phase5.md) | 27–34 | Conferences (read) | Todo |
| 6 | [slices/phase6.md](slices/phase6.md) | 35–36 | Conferences (admin) | Todo |
| 7 | [slices/phase7.md](slices/phase7.md) | 37–41 | Messaging (read) | Todo |
| 8 | [slices/phase8.md](slices/phase8.md) | 42–44 | Messaging (write) | Todo |
| 9 | [slices/phase9.md](slices/phase9.md) | 45–49 | Messaging (advanced) | Todo |
| 10 | [slices/phase10.md](slices/phase10.md) | 50–52 | Files (browse and flag) | Todo |
| 11 | [slices/phase11.md](slices/phase11.md) | 53–57 | Files (transfer) | Todo |
| 12 | [slices/phase12.md](slices/phase12.md) | 58–60 | Files (admin) | Todo |
| 13 | [slices/phase13.md](slices/phase13.md) | 61–64 | Per-user accounting refinements | Todo |
| 14 | [slices/phase14.md](slices/phase14.md) | 65–68 | User self-service | Todo |
| Future | [slices/future.md](slices/future.md) | — | Not yet sliced (FTP, HTTPd, QWK, FTN, OLM, …) | Todo |

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
  toggles slice that exposes them.
- `Node.status` starts as the subset Phase 1 transitions through;
  `reserved`, `suspended` and `shutting_down` land with their commands.
- `PasswordHashKind` starts with one variant (`pbkdf2_10000`, the spec's
  default for new accounts); the legacy 32-bit hash and the lower-round
  pbkdf2 levels arrive when an older user record forces us to read them.
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
  encoding, which we restore via `\u{...}` escapes).
- **Document any deliberate departure** in the slice's In Scope, with
  reasoning. "We renamed X to Y because Z" belongs in the slice spec
  so it isn't quietly drift.

If an asset (`Menu.txt`, `BBSTITLE.txt`, screen file) ships in
`amiexpress/deployment/binaries.lha` (see the asset inventory at the
foot of this file), the adapter loads that asset rather than
rendering a built-in fallback. The fallback exists only for the
"sysop hasn't dropped the file in place yet" case and is built to
look as close to the legacy default as we can make it.

## Progress

A slice is **Done** only when every Allium rule, invariant and black-box
function listed in its "In Scope" section is implemented, backed by tests
that pass, and `cargo test`, `cargo build`, `cargo fmt --check` and
`cargo clippy -- -D warnings` are all clean. Anything else is **Todo**
(or **In progress** while a slice is being worked on).

A **phase** whose theme names a user-facing capability ("Sign in, see
the menu, log off", "Conferences (read)", "Files (transfer)", and so
on) is **Done** only once that capability is reachable by running the
compiled binary — not merely the library or per-test in-process
listeners. Every such phase therefore owns a closing slice that wires
the composition root (`app::main`), pins down the runtime config
acquisition story (config file? built-in defaults? CLI flags?) and the
seed-data story (how does an installer get a user record on disk to
log in as?), and adds a smoke test that spawns the binary process and
exercises the headline flow end-to-end. Library-level slice tests are
necessary but not sufficient: a phase whose binary wouldn't actually
deliver its theme is **In progress**, not Done — even if every named
rule has its own green test. Future-phase slice tables therefore each
end with a "Phase N — wire and smoke" closing slice; the lack of one
in Phase 1 is a planning bug being fixed by Slice 13a below.

| # | Slice | Status |
| ---: | --- | :---: |
| 1 | Cargo crate skeleton | Done |
| 2 | User entity (login-time fields only) | Done |
| 3 | In-memory `UserRepository` port + adapter | Done |
| 4 | Password verification adapter (single algorithm) | Done |
| 5 | Node entity (Phase 1 statuses only) | Done |
| 6 | Session entity skeleton | Done |
| 7 | `AcceptConnection` rule | Done |
| 8 | Telnet listener + per-session task | Done |
| 8a | Telnet wire-quality (echo, password masking, line editing, AmiExpress prompts) | Done |
| 9 | `PromptForName` + `NameTyped` rules (existing user path only) | Done |
| 10 | `VerifyPassword` rule (happy path) | Done |
| 11 | `VerifyPassword` rule (failure path) | Done |
| 12 | `EnterMenu` + display the conference menu | Done |
| 13 | `UserRequestsLogoff` + `FinaliseLogoff` + `ReleaseNode` | Done |
| 13a | Phase 1 wire-and-smoke (composition root + sysop seed) | Done |
| 14 | Daily time budget initialisation + decrement | Done |
| 15 | Forced password reset | Done |
| 16 | Account-locked / insufficient-access rejection | Done |
| 17 | Idle timeout | Done |
| 18 | Carrier loss | Done |
| 19 | `user_typed_NEW` branch | Done |
| 20 | `CompleteNewUserRegistration` | Done |
| 20a | New-user password gate (`VerifyNewUserPassword`) | Done |
| 21 | Pending-validation gate | Done |
| 22 | Sysop direct logon | Todo |
| 23 | Local logon + relogon | Todo |
| 24 | Node reservation | Todo |
| 25 | Node suspend / resume / shutdown | Todo |
| 26 | Sysop kick | Todo |
| 27 | Conference + MessageBase entities | Todo |
| 28 | Conference loader from disk | Todo |
| 29 | `ConferenceMembership` + access checks | Todo |
| 30 | `JoinConference` (auto-rejoin on logon) | Todo |
| 31 | Conference / node bulletins + per-conference menu | Todo |
| 32 | Explicit `J` (join conference) command | Todo |
| 33 | `ConferenceScan` (CS command) | Todo |
| 34 | `JoinedConferenceForNameType` | Todo |
| 35 | Sysop creates conference | Todo |
| 36 | Sysop grants / revokes access | Todo |
| 37 | `Mail` entity + on-disk message store | Todo |
| 38 | `ReadPointers` entity | Todo |
| 39 | `ReadMail` rule + `R` menu command | Todo |
| 40 | `ScanMail` + `M` / `N` menu commands | Todo |
| 41 | Auto mail scan on join | Todo |
| 42 | `PostMail` rule (single-addressee, `E` command) | Todo |
| 43 | Broadcast addressing (ALL / EALL) | Todo |
| 44 | `PostCommentToSysop` (`C` command) | Todo |
| 45 | `ReplyToMail` | Todo |
| 46 | `ForwardMail` | Todo |
| 47 | Censored users + private / private-to-sysop | Todo |
| 48 | `MailAttachment` + `AttachFileToMail` | Todo |
| 49 | `DeleteMail`, `MoveMail`, `EditMailHeader` | Todo |
| 50 | `Bytes` value type + `FileArea` + `File` entities | Todo |
| 51 | `FlagFile` / `UnflagFile` | Todo |
| 52 | `A` (edit file flags) + `Z` (zippy search) commands | Todo |
| 53 | `Transfer` entity + Zmodem adapter (download stub) | Todo |
| 54 | `BeginDownload` + `CompleteDownload` | Todo |
| 55 | `CheckDownloadEligibility` | Todo |
| 56 | `BeginUpload` + `CompleteUpload` | Todo |
| 57 | Background file check | Todo |
| 58 | `SysopUploadFile` | Todo |
| 59 | `MoveFile` + `DeleteFile` + `AdmitHeldFile` | Todo |
| 60 | `lcfiles` and `quarantined` workflows | Todo |
| 61 | Per-conference accounting | Todo |
| 62 | Credit accounts | Todo |
| 63 | Daily byte cap end-to-end | Todo |
| 64 | Legacy + lower-round password hashes | Todo |
| 65 | Quiet mode + ANSI / RIP / expert toggles | Todo |
| 66 | `W` (change user info) command | Todo |
| 67 | `S` (user stats) + `T` (time) commands | Todo |
| 68 | Sysop chat allowance (`O` page sysop) | Todo |

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
| `defaultbbs/Conf01/menu.txt` | Minimal "Lamer Land" 4-command menu (G/O/C/U). Useful as a low-access-tier menu fixture in Slice 31. |
| `defaultbbs/Conf01/path`, `paths`, `NDirs` | Tiny on-disk format for "where does this conference live" — reference for Slice 28 conference loader. |
| `defaultbbs/Conf01/MsgBase/MailStats`, `MailLock` | Seed files showing the message-base on-disk schema; reference for Slice 37 mail store. |
| `defaultbbs/Conf01/Conf.DB`, `defaultbbs/Conf02/Conf.DB` | Empty conference databases; layout reference for Slice 28. |
| `defaultbbs/user.data`, `user.keys`, `user.misc` | Three-file user schema (legacy split). Reference for Slice 3 user repository; the port may collapse to one file. |
| `defaultbbs/SystemStats` | Binary stats template; reference only. |
| `defaultbbs/Documentation/Aedoc4.guide` | Original AmigaGuide manual — search here for any user-facing string we need to mirror exactly (prompts, error wording). |
| `defaultbbs/Access/*.info`, `defaultbbs/Commands/BBSCmd/*.info`, `defaultbbs/FCheck/*.info`, `defaultbbs/Protocols/Xpr*.info` | Amiga tooltype configs. Reference only — per `AGENTS.md`, the Rust port stores config in files, not icon tooltypes. |
| `amiexpress/express.e:6539` (`displayScreen`) | Authoritative list of which SCREEN_* names the BBS dispatches and the order they appear in. |
