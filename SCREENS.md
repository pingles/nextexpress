# SCREENS.md

Catalogue of every BBS screen the legacy AmiExpress source can render
and how NextExpress treats it. The original `SCREEN_*` enum lives at
`amiexpress/axenums.e:19`; the dispatch table that turns enum values
into filesystem paths is `displayScreen` at `amiexpress/express.e:6539`.

For each screen this file records:

- **Filesystem name** the adapter looks for (typically under `Screens/`
  or `ConfXX/`; baud-suffixed for the per-baud entries).
- **Trigger** — what user action or session state causes the screen.
- **Allium rule** the screen is presentation for, when one exists.
- **Display semantics** — pure-cosmetic (skipped silently when missing),
  pause-after (only pauses if the asset rendered), gate-by-presence
  (the asset's existence enables a behaviour), or fall-back (built-in
  text replaces the screen when missing).
- **Built-in fall-back** — what NextExpress emits when the asset is
  missing. `none` means "skip silently"; otherwise the literal bytes
  (CRLF-terminated, ANSI escapes preserved) the adapter writes.
- **Source** — the `amiexpress/express.e:LINE` that displays the screen
  in the legacy code. Use this for parity audits.

Adapters that load these assets MUST normalise Amiga `\b\n` line
endings to `\r\n` before transmission and MUST silently skip any
`displayScreen()` call whose asset isn't on disk unless the screen is
marked **Gate-by-presence**, in which case absence is observable
behaviour.

Screens are ordered by where they appear in the user's journey:
pre-connect → registration → authentication → menu → file ops →
logoff. Conference / messaging / file screens that haven't landed in
NextExpress yet are listed at the end with their expected slice.

---

## Pre-connect

### `BBSTITLE`

- **Trigger:** every accepted connection, immediately after telnet
  IAC negotiation.
- **Allium rule:** `session.allium:AcceptConnection` (presentation
  consequent).
- **Display semantics:** pure-cosmetic banner.
- **Built-in fall-back:** `NextExpress\r\n` (one literal line; see
  `rust/src/adapters/file_screen_repository.rs::FALLBACK_BANNER`).
  The two AmiExpress / NextExpress copyright lines are *always*
  appended afterwards regardless of whether the screen rendered.
- **Source:** `amiexpress/express.e:29552`.

### `AWAIT` (`AWAITSCREEN.TXT`)

- **Trigger:** while a node is idle and the BBS is waiting on the
  console for the sysop's F1 / F2 keypress (legacy local-console
  workflow).
- **Allium rule:** none — the await screen is a console-side aid for
  the sysop, not a domain event.
- **Display semantics:** fall-back to a built-in F-key menu.
- **Built-in fall-back:** the legacy adapter calls `displayKeys()` which
  prints the F1-F4 hot-key menu. NextExpress is headless by default,
  so this screen has no analogue in the modern adapter and is **not
  modelled**.
- **Source:** `amiexpress/express.e:29926`.

### `LOGON24` (`Logon24hrs`)

- **Trigger:** authenticated user has burned through their daily time
  budget before the menu loop starts.
- **Allium rule:** `session.allium:TimeExpired` (the at-logon variant
  — TimeExpired today fires only at runtime; an at-logon check is on
  the Phase 4 backlog).
- **Display semantics:** fall-back text + immediate disconnect.
- **Built-in fall-back:** `You have exceeded your time limit\r\nGoodbye\r\nDisconnecting..\r\n`
  (three literal lines, sent in order).
- **Source:** `amiexpress/express.e:558`.

### `NOCALLERSATBAUD` (`NOCALLERSAT<baud>`, e.g. `NOCALLERSAT9600`)

- **Trigger:** caller's baud rate is restricted from connecting at all.
  The screen file's *presence* is the gate; the suffix encodes the
  baud rate so different rates can carry different explanations.
- **Allium rule:** none yet — modern transports don't have a baud
  rate, so this gate currently maps to no NextExpress behaviour.
- **Display semantics:** gate-by-presence + immediate disconnect.
- **Built-in fall-back:** none (if the screen file is absent the gate
  isn't enforced).
- **Source:** `amiexpress/express.e:29486`.

### `NOT_TIME` (`NOTTIME<baud>`)

- **Trigger:** requested baud rate is blocked during the current time
  period (e.g. only 2400-baud allowed during peak hours).
- **Allium rule:** none yet — same reasoning as `NOCALLERSATBAUD`.
- **Display semantics:** fall-back text + 2-second `Delay(120)` pause +
  disconnect.
- **Built-in fall-back:** `\r\n<baud> baud is not allowed at this time.\r\n\r\n`
  (with `<baud>` substituted for the connection's baud rate).
- **Source:** `amiexpress/express.e:29301`.

### `ONENODE` (`OnlyOnOneNode`)

- **Trigger:** authenticated user is already logged in on another node
  (multi-node concurrency enforcement).
- **Allium rule:** none yet — the equivalent invariant is
  `OneActiveSessionPerNode` in `session.allium`, but that's per-node
  not per-user. A `RejectConcurrentSession` rule is on the Phase 4
  backlog.
- **Display semantics:** fall-back text + immediate disconnect.
- **Built-in fall-back:** `\r\nYou are already logged into another node!\r\n`.
- **Source:** `amiexpress/express.e:29719`.

### `PRIVATE`

- **Trigger:** sysop console logon (F1) — displayed before the system
  password prompt that the sysop must still satisfy.
- **Allium rule:** `session.allium:SysopDirectLogon` (presentation
  consequent; see the rule's `@guidance`).
- **Display semantics:** pure-cosmetic.
- **Built-in fall-back:** `none`. The system-password prompt that
  follows is *not* modelled in the spec — it is a deployment-secret
  guard outside per-user authentication.
- **Source:** `amiexpress/express.e:29336`.

---

## Authentication / lockout

### `LOCKOUT0`, `LOCKOUT1`

- **Trigger:** authenticated user is locked out
  (`access_level <= 1` or `account_locked`). `LOCKOUT0` is rendered
  for `access_level = 0` (no access at all); `LOCKOUT1` for
  `access_level = 1` (nearly locked).
- **Allium rule:** `session.allium:RejectLockedOrInsufficientAccess`.
- **Display semantics:** pure-cosmetic before the `LoggingOff`
  transition completes.
- **Built-in fall-back:** `Logon rejected. Goodbye.\r\n` (one literal
  line; see `rust/src/adapters/telnet_listener.rs`).
- **Source:** `amiexpress/express.e:29770`.

### `LOGON`

- **Trigger:** every successful authentication, immediately after
  the password match and before the `EnterMenu` rule.
- **Allium rule:** `session.allium:EnterMenu` (presentation
  consequent).
- **Display semantics:** pause-after — `IF displayScreen(SCREEN_LOGON)
  THEN doPause()`. Skipped silently when missing.
- **Built-in fall-back:** `Authenticated.\r\n` (the current Phase 1
  acknowledgement; will move to a richer post-LOGON flow in later
  slices).
- **Source:** `amiexpress/express.e:29854`.

---

## New-user registration

### `NONEWUSERS`

- **Trigger:** typed handle was the literal `NEW`, but the BBS does
  not accept new registrations
  (`core/config.allow_new_users = false`).
- **Allium rule:** `session.allium:RejectDisallowedRegistration`.
- **Display semantics:** gate-by-presence (legacy) — the legacy code
  takes the file's existence as the signal to refuse registration. In
  NextExpress the explicit `allow_new_users` flag is the gate; the
  asset is the explanatory screen rendered when the flag rejects.
- **Built-in fall-back:** `New user registration is not available. Goodbye.\r\n`.
- **Source:** `amiexpress/express.e:30008`.

### `NONEWATBAUD` (`NONEWAT<baud>`)

- **Trigger:** typed handle was the literal `NEW`, registration is
  globally allowed, but the caller's baud rate is restricted from
  registering. As with `NONEWUSERS` the file's presence is the gate.
- **Allium rule:** none yet — modern transports don't have baud
  rates. Maps to the same `RejectDisallowedRegistration` rule when
  enabled.
- **Display semantics:** gate-by-presence + disconnect.
- **Built-in fall-back:** none (gate disabled when no asset).
- **Source:** `amiexpress/express.e:30010`.

### `NEWUSERPW`

- **Trigger:** typed handle was the literal `NEW`, registration is
  allowed, and the sysop has set
  `core/config.new_user_password` (non-null). The screen explains the
  gate; the prompt that follows verifies the password.
- **Allium rule:** `session.allium:InitialiseNewUserGate` (display) and
  `session.allium:VerifyNewUserPassword` (the prompt + retry loop).
- **Display semantics:** pure-cosmetic above the prompt; the
  prompt-and-retry-loop is the actual gate.
- **Built-in fall-back:** `\r\nNew user registration.\r\n` for the
  screen, then the prompt `Enter New User Password: ` (verbatim from
  `amiexpress/express.e:30018`). On a failure the adapter writes
  `Invalid PassWord\r\n` (verbatim from `amiexpress/express.e:30036`)
  and re-prompts. After the configured retry budget
  (`core/config.max_new_user_password_attempts`, default 3) the
  adapter writes `\r\nExcessive Password Failure\r\n` (verbatim from
  `amiexpress/express.e:30039`) and disconnects.
- **Source:** `amiexpress/express.e:30014` (screen) and
  `:30018` (prompt).

### `GUESTLOGON`

- **Trigger:** the new-user password gate has either passed or was
  not configured; the registration form is about to be offered.
- **Allium rule:** part of the adapter sequence around
  `session.allium:CompleteNewUserRegistration` (see that rule's
  `@guidance`).
- **Display semantics:** pause-after — `IF displayScreen(...) THEN doPause()`.
- **Built-in fall-back:** none. The registration form prompts that
  follow are the only required output:

  | Field | Prompt | Source |
  | --- | --- | --- |
  | Handle | `\r\nEnter your Name: ` | `express.e:30141` |
  | Location | `City, State: ` | `express.e:30194` |
  | Phone | `Phone Number: ` | `express.e:30204` |
  | Email | `E-Mail Address: ` | `express.e:30215` |
  | Password | `Enter a PassWord: ` | `express.e:30227` |
  | Confirm | `Reenter the PassWord: ` | `express.e:30233` |
  | Line length | `Enter line length (or 0 for Auto): ` | simplified from `express.e:11307` |
  | ANSI | `Use ANSI graphics? (Y/n) ` | simplified from `express.e:29528`'s `ANSI, RIP or No graphics (A/r/n)?` |

  Mismatched passwords trigger `\r\nPasswords do not match, try again..\r\n`
  (verbatim from `express.e:30237`); a duplicate or reserved handle
  triggers `That name is taken. Try another.\r\n`.
- **Source:** `amiexpress/express.e:30049`.

### `JOIN`

- **Trigger:** during new-user account finalisation, before the
  per-conference `JoinConference` sub-flow runs for the first time.
- **Allium rule:** part of the adapter sequence around
  `session.allium:CompleteNewUserRegistration` (see that rule's
  `@guidance`); the sub-flow itself lands with `conferences.allium`
  in Phase 5.
- **Display semantics:** pause-after.
- **Built-in fall-back:** none.
- **Source:** `amiexpress/express.e:30057`.

### `JOINED`

- **Trigger:** new-user account creation has succeeded and the
  session is about to enter the menu loop. The final new-user
  welcome.
- **Allium rule:** post-`session.allium:CompleteNewUserRegistration`
  presentation; see that rule's `@guidance`.
- **Display semantics:** pause-after.
- **Built-in fall-back:** `\r\nWelcome aboard!\r\n` (see
  `rust/src/adapters/telnet_listener.rs::REGISTRATION_COMPLETE_LINE`).
- **Source:** `amiexpress/express.e:30125`.

---

## Menu / on-logon

### `MENU` (per-conference; user's `currentMenuName` or
`defaultMenuName`)

- **Trigger:** every iteration of the menu loop until the user
  invokes a command that takes them away from the menu.
- **Allium rule:** `session.allium:EnterMenu` — the menu screen IS
  the presentation consequent of being in the `menu` state.
- **Display semantics:** required — unlike most other screens, an
  absent menu file falls back to a built-in.
- **Built-in fall-back:** `[ Default menu - type G to log off ]\r\n`
  (see `rust/src/adapters/file_screen_repository.rs::FALLBACK_MENU`).
  The Phase 1 default; the per-conference Slice 28 work will load
  `Conf02/Menu.txt` when present.
- **Source:** `amiexpress/express.e:24597` and `:28586`.

### `BULL`, `NODE_BULL`, `CONF_BULL`

- **Trigger:** between successful authentication and `EnterMenu`. The
  three are displayed in sequence (system bulletin, then node-scoped,
  then conference-scoped).
- **Allium rule:** none yet — bulletins land with the per-conference
  Phase 5 work; the spec acknowledges them in `EnterMenu`'s
  `@guidance`.
- **Display semantics:** pause-after each.
- **Built-in fall-back:** none — bulletins are sysop-supplied or
  absent.
- **Source:** `amiexpress/express.e:28556` (BULL), `:28557`
  (NODE_BULL), `:5058` (CONF_BULL).

### `MAILSCAN`

- **Trigger:** between bulletins and the menu, before the optional
  new-mail scan prompt.
- **Allium rule:** none yet — auto mail scan lands with Slice 41
  (`messaging.allium`).
- **Display semantics:** pure-cosmetic; the adapter checks the
  `MAILSCAN_PROMPT` tooltype (NextExpress: a config key) for the
  optional prompt that follows.
- **Built-in fall-back:** none.
- **Source:** `amiexpress/express.e:28073`.

---

## Conferences

### `JOINCONF`

- **Trigger:** user invokes the `J` (join conference) command.
- **Allium rule:** `conferences.allium:JoinConference` (Slice 32).
- **Display semantics:** pure-cosmetic above the conference number
  prompt.
- **Built-in fall-back:** none.
- **Source:** `amiexpress/express.e:25143`.

### `CONF_JOINMSGBASE`, `JOINMSGBASE`

- **Trigger:** user is selecting a message base within the current
  conference (when more than one exists). `CONF_JOINMSGBASE` is the
  conference-scoped variant; on miss the adapter falls through to the
  node-wide `JOINMSGBASE`.
- **Allium rule:** Phase 5 / Phase 7 messaging slices.
- **Display semantics:** cascading fallback.
- **Built-in fall-back:** none beyond the cascade.
- **Source:** `amiexpress/express.e:25170-25171`, `:25221-25222`.

---

## File operations

### `DOWNLOAD` (`DownloadMsg`)

- **Trigger:** before a file download begins.
- **Allium rule:** `files.allium:BeginDownload` (Slice 54).
- **Display semantics:** pause-after + a trailing newline.
- **Built-in fall-back:** none.
- **Source:** `amiexpress/express.e:19967`.

### `UPLOAD` (`UploadMsg`)

- **Trigger:** before a file upload begins (when not auto-detecting
  upload type).
- **Allium rule:** `files.allium:BeginUpload` (Slice 56).
- **Display semantics:** pure-cosmetic.
- **Built-in fall-back:** none.
- **Source:** `amiexpress/express.e:18986`.

### `NOUPLOADS` (`NoUploads`)

- **Trigger:** user attempts upload but the conference has uploads
  disabled.
- **Allium rule:** `files.allium:BeginUpload` (rejection branch,
  Slice 56).
- **Display semantics:** display + immediate exit (`RESULT_SUCCESS`
  in legacy).
- **Built-in fall-back:** none in the legacy code; the modern adapter
  will likely emit a built-in line so the rejection is unambiguous.
- **Source:** `amiexpress/express.e:18981`.

### `FILEHELP` (`FileHelp`)

- **Trigger:** user requests help during file browsing.
- **Allium rule:** `files.allium` help command (Phase 10).
- **Display semantics:** fall-back text.
- **Built-in fall-back:** `unAvailNotice(GSTR3, GSTR1)` — a
  translator-resolved string in the legacy code; NextExpress will
  carry a literal "Help unavailable.\r\n" until a richer help system
  lands.
- **Source:** `amiexpress/express.e:27646`.

---

## Real / internet names

### `REALNAMES` (`RealNames`)

- **Trigger:** posting a message in a conference whose
  `accepted_name_type = real_name`, when the user has no
  `real_name` on file.
- **Allium rule:** `messaging.allium:PostMail` precondition (Phase 8).
- **Display semantics:** fall-back text + a 5-attempt input loop.
- **Built-in fall-back:**
  `Real Names are required for messages in this conference/msgbase \r\n`
  (verbatim from `express.e:28169`).
- **Source:** `amiexpress/express.e:28169`.

### `INTERNETNAMES` (`InternetNames`)

- **Trigger:** posting a message in a conference whose
  `accepted_name_type = internet_name`, when the user has no
  `internet_name` on file.
- **Allium rule:** `messaging.allium:PostMail` precondition (Phase 8).
- **Display semantics:** fall-back text + input loop.
- **Built-in fall-back:**
  `Internet Names are required for messages in this conference/msgbase\r\n`
  (verbatim from `express.e:28199`).
- **Source:** `amiexpress/express.e:28199`.

---

## Multi-language

### `LANGUAGES` (`Languages`)

- **Trigger:** user invokes the language-change command.
- **Allium rule:** none — the multi-language translator is on the
  `slices/future.md` backlog.
- **Display semantics:** fall-back text + numeric prompt loop.
- **Built-in fall-back:** `Languages list unavailable\r\n\r\n`
  (verbatim from `express.e:11395`).
- **Source:** `amiexpress/express.e:11395`.

---

## Logoff

### `LOGOFF`

- **Trigger:** every clean logoff (`normal_logoff`), immediately
  before the transport closes.
- **Allium rule:** `session.allium:FinaliseLogoff` (presentation
  consequent; see `@guidance`).
- **Display semantics:** pure-cosmetic; not rendered for FTP
  channels (`is_remote = ftp`) or for sysop console.
- **Built-in fall-back:** `Goodbye!\r\n` (see
  `rust/src/adapters/telnet_listener.rs::GOODBYE_LINE`). For non-clean
  exits the adapter writes the relevant goodbye line in lieu of the
  screen: `Idle timeout. Goodbye.\r\n`, `Account locked. Goodbye.\r\n`,
  `Too many password failures. Goodbye.\r\n`, etc. Carrier-loss exits
  write nothing — the connection is already gone.
- **Source:** `amiexpress/express.e:8187`.

---

## Tracking matrix

This matrix is the quick-look "is the screen wired up in NextExpress
yet" view. Adapter status reflects current `rust/` state; rule status
reflects the spec.

| Screen | Adapter status | Spec rule | Slice |
| --- | :---: | --- | --- |
| BBSTITLE | implemented | `AcceptConnection` | 8 |
| AWAIT | n/a (headless) | — | — |
| LOGON24 | not yet | (TimeExpired-at-logon, planned) | 14 polish |
| NOCALLERSATBAUD | not yet | (no rule, baud-vestigial) | — |
| NOT_TIME | not yet | (no rule, baud-vestigial) | — |
| ONENODE | not yet | (RejectConcurrentSession, planned) | Phase 4 |
| PRIVATE | not yet | `SysopDirectLogon` | 22 |
| LOCKOUT0 / LOCKOUT1 | adapter has fall-back line | `RejectLockedOrInsufficientAccess` | 16 |
| LOGON | adapter has fall-back line | `EnterMenu` (presentation) | 12 |
| NONEWUSERS | not yet | `RejectDisallowedRegistration` | Phase-3 follow-up |
| NONEWATBAUD | not yet | `RejectDisallowedRegistration` | Phase-3 follow-up |
| NEWUSERPW | screen rendered with fall-back; gate not yet | `InitialiseNewUserGate` + `VerifyNewUserPassword` | Phase-3 follow-up |
| GUESTLOGON | not yet | (CompleteNewUserRegistration adapter sequence) | 20 follow-up |
| JOIN | not yet | (CompleteNewUserRegistration adapter sequence) | 20 follow-up |
| JOINED | adapter has fall-back line ("Welcome aboard!") | (CompleteNewUserRegistration adapter sequence) | 20 |
| MENU | implemented (with Phase-1 fall-back) | `EnterMenu` | 12 |
| BULL / NODE_BULL / CONF_BULL | not yet | (Phase 5 / 7) | 31 |
| MAILSCAN | not yet | (Slice 41) | 41 |
| JOINCONF | not yet | `JoinConference` | 32 |
| CONF_JOINMSGBASE / JOINMSGBASE | not yet | (Phase 5) | 28 |
| DOWNLOAD | not yet | `BeginDownload` | 54 |
| UPLOAD | not yet | `BeginUpload` | 56 |
| NOUPLOADS | not yet | `BeginUpload` (rejection) | 56 |
| FILEHELP | not yet | (Phase 10) | 52 |
| REALNAMES | not yet | `PostMail` precondition | Phase 8 |
| INTERNETNAMES | not yet | `PostMail` precondition | Phase 8 |
| LANGUAGES | not yet | (future.md) | — |
| LOGOFF | adapter has fall-back line ("Goodbye!") | `FinaliseLogoff` | 13 |
