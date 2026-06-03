# Slice C5 — `CF` (Conference Flags Editor): Design & Plan

Date: 2026-06-03
Status: design approved in principle; legacy behaviour validated live; awaiting
spec-doc review before writing the implementation plan.

## 1. Context

Tier C (conference navigation) was re-ordered so `CF` (slice C5) lands **before**
`CS` (slice C1). The reason: a faithful `CS` (on-demand multi-conference mail
scan) is byte-for-byte identical to the already-shipped `MS` until per-base scan
flags exist, and those flags (`ConferenceMembership.mail_scan` et al.) are
introduced by `CF`. So `CF` is the unblocking slice.

`CF` lets the logged-on user edit **their own** per-conference scan preferences —
which conferences are swept for new mail (`M`), all-messages (`A`), new files
(`F`), and ZOOM/QWK gather (`Z`). Legacy: `internalCommandCF`,
`amiexpress/express.e:24672-24841`. It is a personal-preferences command (not a
sysop-edits-others command), gated by `ACS_CONFFLAGS`.

## 2. Legacy behaviour assessment (validated live)

Validated against the FS-UAE AmiExpress reference (`amiexpress-ref`,
AmiExpress 5.6.0) over telnet on 2026-06-03. Driver: `/tmp/cf_probe2.py`
(pause-aware); raw transcript: `/tmp/cf2.out`. Login `sysop`/`sysop` (sec 255);
seed has two single-msgbase conferences — `1 New Users`, `2 Amiga`.

### Method notes (harness)

- The graphics prompt is `lineInput` — send `A\r` (bare `A` hangs).
- **Post-login paginates**: `Scanning conferences for mail...` then
  `(Pause)...Space To Resume:` — must answer with a space or the session
  desyncs (this stalled the first probe attempt).
- Every session MUST end `G Y` (FS-UAE spin-wait hazard); an unclean kill leaves
  a phantom `sysop` login that blocks re-login — restart the container to clear.

### Captured wire facts (exact bytes)

| Element | Exact bytes (Python repr) | Source |
| --- | --- | --- |
| Clear screen | `\x0c` | `sendCLS`, `express.e:5224` (byte 12) |
| Header row 1 | `\x1b[32m        M A F Z Conference                      M A F Z Conference\x1b[0m\r\n` | `:24691` |
| Header row 2 | `\x1b[33m        ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~         ~ ~ ~ ~ ~~~~~~~~~~~~~~~~~~~~~~~\x1b[0m\r\n` | `:24692` |
| Blank line after header | `\r\n` | `:24690`/`:24692` |
| Conference entry (single-base) | `\x1b[34m[\x1b[0m` + `%5d`(num) + `\x1b[34m] \x1b[36m` + `M A F Z ` (each glyph + space, 8 chars) + `\x1b[0m` + `%-23s`(name) | `:24735` |
| Entry separator (even index) | single space ` ` | `:24742` (`n AND 1` false) |
| Row terminator (odd index) | `\r\n` | `:24740` |
| Pre-prompt gap | `\r\n\r\n` | `:24749` (`\b\n\b\n`) |
| Mask prompt | `Edit which flags [M]ailScan, [A]ll Messages, [F]ileScan, [Z]oom >: ` | `:24749` |
| Mask key echo | `<char>\r\n` | `:24763-24766` |
| Expression prompt | `Enter Conference Numbers,'*' toggle all,'-' All off,'+' All on >: ` | `:24769` |
| Logoff side effect | `** AutoSaving File Flags **` | flags persist on logoff |

Column order on the wire is **M A F Z = c1 c4 c2 c3** = MAIL_SCAN(4),
MAILSCAN_ALL(128), FILE_SCAN(8), ZOOM_SCAN(2) (`axconsts.e:45-48`). Conference
number is right-justified width 5; name left-justified width 23; two conferences
per line.

### Behavioural findings (claim → observed)

| Behaviour | Expectation from source | Live result |
| --- | --- | --- |
| Single conf number (`2`) | EOR-toggles that conf's selected flag | ✓ conf 2 `M` flipped ` `→`*` |
| `-` (all off) | clears the flag on every accessible conf | ✓ conf 2 `M` `*`→` ` |
| `+` (all on) | sets the flag on every accessible conf | not re-run live; source unambiguous (`OR editmask`) |
| **`*` (toggle all)** | **prompt advertises it but code has NO `*` branch** | ✓ **NO-OP confirmed** — flags unchanged after `M *` |
| Exit | any non-M/A/F/Z key at the mask prompt exits | ✓ `Q` returned to the menu |
| Empty expression | returns to the menu immediately | source: `StrLen=0 → RETURN` (not re-run) |
| Default flags | seeded confs show **all blank** (no DEFAULT_NEWSCAN tooltype) | ✓ both confs all-blank |

**The headline finding** is the `*` no-op: the legacy expression prompt
advertises `'*' toggle all`, but `internalCommandCF` has only `+`, `-`, and the
comma-list branches — `*` falls through to the conf-number parser, matches no
conference, and does nothing. This is a legacy bug; the prompt's contract is
unfulfilled.

## 3. Design

### Approach

Hexagonal split matching the existing code (`MS` = `scan_all_mail` use case +
`menu_flow` handler; `R` sub-prompt = `read_subprompt` loop): pure-domain edit
logic, a terminal-free listing use case, and a thin terminal loop.

### 3.1 Spec (Allium) — via `allium:tend`

- `core.allium` `ConferenceMembership`: add `mail_scan`, `mailscan_all`,
  `file_scan`, `zoom_scan : Boolean`. Defaults on a **granted** row:
  `mail_scan = true`, `file_scan = true`, others `false`.
- `conferences.allium`: a rule `EditConferenceScanFlags` capturing set / clear /
  toggle semantics, and the `mail_scan` gate the later `CS` slice consumes.
- A `has_access` right for the `ACS_CONFFLAGS` gate (`Right::EditConferenceFlags`).

### 3.2 Rust schema

Add the four booleans to `ConferenceMembership` (defaults as above), a `ScanFlag`
enum (`MailScan`/`MailScanAll`/`FileScan`/`Zoom`) carrying the legacy mask values
(4/128/8/2) in parity comments, and get/set/toggle accessors.

### 3.3 Edit semantics (pure domain)

`apply_scan_flag_edit(memberships, flag, expression)` where `expression` ∈:
- `+` → set the flag on every accessible membership
- `-` → clear it on every accessible membership
- `*` → toggle it on every accessible membership *(see Departure D1)*
- comma-list of conference numbers → toggle each named conference

Unit-tested independently of any terminal.

### 3.4 Listing use case (terminal-free)

`conf_flags_listing(memberships, conferences)` → render-ready rows: per accessible
conference, the four glyph cells (`*`/` `) + the right-justified number + the
left-justified name. Returns data; the handler renders bytes.

### 3.5 Wire rendering

Reproduce the captured bytes verbatim (section 2 table), each constant carrying a
`// amiexpress/express.e:NNNN` comment. CLS = `0x0C`. `F`/`D` tooltype-override
glyphs are out of scope (slice C5 defers them) — cells render only `*` or space.

### 3.6 Terminal loop (`menu_flow/conf_flags.rs`)

`CLS → render listing → readChar mask (M/A/F/Z; any other key exits) → echo char
+ CRLF → lineInput expression (empty exits) → apply via the domain fn → repeat.`
Same shape as `read_subprompt`.

### 3.7 Parser + menu

`MenuCommand::ConferenceFlags` for `CF` (rejects extra tokens); add `CF` to
`Conf02/Menu5.txt` (CONFERENCES section) and to the `advertised_token` /
`every_menu_command` guard helpers.

### 3.8 Access gate

`CF` edits the caller's own preferences, so `Right::EditConferenceFlags` is granted
broadly by default — the gate exists for parity but locks no normal user out.

### 3.9 Storage granularity

Per-conference on `ConferenceMembership`. Every shipped conference (legacy seed
and NextExpress) has a single message base, so this is observationally identical
to the legacy per-(conf,msgbase) `cb.handle[0]` storage. Per-base flags for
multi-base conferences are a future extension, not built.

### 3.10 Deliberate departures (documented)

- **D1 — `*` toggle-all.** The legacy advertises `'*' toggle all` but no-ops it
  (validated live). NextExpress implements `*` as the advertised toggle-all,
  honouring the prompt's contract over a legacy bug.
- **D2 — default flags ON.** The legacy seed shows all flags blank (no
  `DEFAULT_NEWSCAN`/`DEFAULT_NEW_FILES` tooltypes, which we scope out as
  file-config). NextExpress defaults `mail_scan`/`file_scan` ON for a granted
  membership so `CS`/`N` work out of the box. (User decision, 2026-06-03.)

## 4. Testing

- Pure-domain unit tests for `apply_scan_flag_edit` (each of `+`/`-`/`*`/list;
  the `*` toggle-all departure pinned explicitly).
- Use-case tests for the listing rows (glyph cells, widths, ordering).
- Wire tests pinning the exact header/row/prompt bytes from section 2.
- A telnet smoke: log in → `CF` → toggle a conf → confirm the `*` shows on
  re-display and persists.
- `cargo mutants` on the changed files.

## 5. Implementation plan (slice breakdown)

The single C5 slice is delivered as ordered TDD cycles:

1. **Spec** — `allium:tend`: add the four flags + defaults + `EditConferenceScanFlags`
   + the access right to `core.allium`/`conferences.allium`.
2. **Schema** — `ConferenceMembership` fields, defaults, `ScanFlag` enum, accessors.
3. **Edit semantics** — `apply_scan_flag_edit` (pure domain), all four expression forms.
4. **Listing use case** — `conf_flags_listing` render-ready rows.
5. **Wire rendering** — verbatim header/row/prompt constants + the row formatter.
6. **Terminal loop** — `menu_flow/conf_flags.rs` (CLS, render, mask, expression, repeat).
7. **Parser + menu** — `MenuCommand::ConferenceFlags`, `Menu5.txt`, guard helpers.
8. **Access gate** — `Right::EditConferenceFlags` + `has_access` mapping.
9. **Wire-and-smoke** — telnet smoke end-to-end; `cargo mutants`; update `SLICES.md`
   (C5 done, CS unblocked) and `SYSTEM.md`.

## 6. Out of scope

- `F`/`D` per-conference newscan tooltype overrides (file-config, deferred).
- Per-message-base flags for multi-base conferences (future extension).
- The `CS` command itself (next slice, now unblocked by C5's `mail_scan`).

## 7. Decisions on record

- Storage: per-conference (§3.9).
- `*`: implement as toggle-all, documented departure (§3.10 D1).
- Defaults: `mail_scan`/`file_scan` ON (§3.10 D2; user, 2026-06-03).
- Gate: broadly granted (§3.8).
