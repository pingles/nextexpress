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

| Phase | File | Slices | Theme |
| --- | --- | --- | --- |
| 0 | [slices/phase0.md](slices/phase0.md) | 1 | Project foundations |
| 1 | [slices/phase1.md](slices/phase1.md) | 2–13, 13a | Sign in, see the menu, log off |
| 2 | [slices/phase2.md](slices/phase2.md) | 14–18 | Hardening the logon flow |
| 3 | [slices/phase3.md](slices/phase3.md) | 19–21 | New user onboarding |
| 4 | [slices/phase4.md](slices/phase4.md) | 22–26 | Sysop console & node controls |
| 5 | [slices/phase5.md](slices/phase5.md) | 27–34 | Conferences (read) |
| 6 | [slices/phase6.md](slices/phase6.md) | 35–36 | Conferences (admin) |
| 7 | [slices/phase7.md](slices/phase7.md) | 37–41 | Messaging (read) |
| 8 | [slices/phase8.md](slices/phase8.md) | 42–44 | Messaging (write) |
| 9 | [slices/phase9.md](slices/phase9.md) | 45–49 | Messaging (advanced) |
| 10 | [slices/phase10.md](slices/phase10.md) | 50–52 | Files (browse and flag) |
| 11 | [slices/phase11.md](slices/phase11.md) | 53–57 | Files (transfer) |
| 12 | [slices/phase12.md](slices/phase12.md) | 58–60 | Files (admin) |
| 13 | [slices/phase13.md](slices/phase13.md) | 61–64 | Per-user accounting refinements |
| 14 | [slices/phase14.md](slices/phase14.md) | 65–68 | User self-service |
| Future | [slices/future.md](slices/future.md) | — | Not yet sliced (FTP, HTTPd, QWK, FTN, OLM, …) |

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
| 9 | `PromptForName` + `NameTyped` rules (existing user path only) | Done |
| 10 | `VerifyPassword` rule (happy path) | Done |
| 11 | `VerifyPassword` rule (failure path) | Done |
| 12 | `EnterMenu` + display the conference menu | Done |
| 13 | `UserRequestsLogoff` + `FinaliseLogoff` + `ReleaseNode` | Done |
| 13a | Phase 1 wire-and-smoke (composition root + sysop seed) | Done |
| 14 | Daily time budget initialisation + decrement | Todo |
| 15 | Forced password reset | Todo |
| 16 | Account-locked / insufficient-access rejection | Todo |
| 17 | Idle timeout | Todo |
| 18 | Carrier loss | Todo |
| 19 | `user_typed_NEW` branch | Todo |
| 20 | `CompleteNewUserRegistration` | Todo |
| 21 | Pending-validation gate | Todo |
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
