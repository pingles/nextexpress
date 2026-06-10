# Live observations — AmiExpress 5.6.0 reference, Tier D file listings

Captured 2026-06-10 against the genuine AmiExpress 5.6.0 binary
(FS-UAE Docker harness, `docker/amiexpress-fsuae`). Board fixture for
these sessions:

- `Conf02.info` byte-patched `NDIRS=1` → `NDIRS=2` (same-length tooltype
  substitution; `maxDirs` is read from the conference `NDIRS` tooltype,
  `express.e:5006`).
- `Conf02/Dir1` seeded with 28 entries and `Conf02/Dir2` with 3, written
  in the authentic upload-writer row format (`express.e:19450-19509`,
  size via `formatFileSizeForDirList` `:18918`, date via `formatLongDate`
  `MiscFuncs.e:278`, FORMAT_USA → `MM-DD-YY`): filename `\l\s[13]`,
  status char at col 13 (`P`assed / `F`ailed / `N`ot-allowed / `D`upe)
  when the name is <13 chars, size `\r\d[7]` at col 14 (unpadded when
  >9,999,999 — deliberate seed `MEGADEMO.DMS` 12345678), date at col 23,
  description at col 33, continuation lines indented 33 spaces, optional
  `Sent by: <name>` continuation (SENTBY_FILES, `:19506`). Deliberate
  edge-case rows: `THIRTEENCH.LZ` (13-char name → no status char),
  `ALONGFILENAME.LHA` (>13 chars → columns shift right). Exact fixture
  bytes preserved in [`fixtures/Dir1`](fixtures/Dir1) /
  [`fixtures/Dir2`](fixtures/Dir2).
- `Conf01/Dir1` left empty (0 bytes). Four blobs with exact listed sizes
  dropped in `Conf02/Uploads/` for future transfer-tier work.
- The FTP daemon's DIR parser (`ftpd.e:1093-1132`) independently
  confirms the row offsets: size at +14, date at +23 (8 chars,
  `XX-XX-XX`), description at +33.

Raw transcripts (every block has RENDER + Python-repr byte forms):

| Transcript | Session |
| --- | --- |
| `comparison/transcripts/ae_tierd_aquascan_accidental.txt` | First contact — battery written for the stock pager collided with AquaScan's; most blocks after F1 are cascade-contaminated but the F 1 listing + `More?` capture is genuine |
| `comparison/transcripts/ae_tierd_aquascan.txt` | Deliberate AquaScan pass; discovered the door's own sub-prompts (some later blocks cascade-shifted) |
| `comparison/transcripts/ae_tierd_aquascan3.txt` | **Cleanest AquaScan evidence** — surgical pass, per-prompt answer queues, 10/12 scenarios clean, 2 self-recovered |
| `comparison/transcripts/ae_tierd_stock.txt` | Stock internal F first pass — F1 block is the canonical raw-stream capture; everything after bounced off the unanticipated `(Pause)` prompt |
| `comparison/transcripts/ae_tierd_stock2.txt` | Stock internal corrected pass — T1–T14 captured (some with recovery noise); T15 onward died in internal `N`'s looping date prompt |

## The headline discovery: door icons shadow the internal commands

The stock deployment ships **AquaScan v1.0 by Aquarius/Outlaws** door
icons in `BBS:Commands/BBSCmd/` for `CS`, `F`, `FR`, `N`, `NSU`, `SCAN`,
`SENT`. `processCommand` (`express.e:28229-28256`) dispatches SYSCmd
icons, then BBSCmd icons, then `processInternalCommand` (`:28285`) —
so on the board as shipped, **typing `F`/`FR`/`N` runs AquaScan and the
internal listers never execute**. (`SENT` is a different door again:
"File Description v1.1 ©1994 Bobo of Mystic", a full-screen ANSI
editor.) No Tier A/B/C command token has a door icon, so all earlier
captures are genuine internals.

**Decision (Paul, 2026-06-10): NextExpress's Tier D parity target is the
AquaScan experience** — users of the reference board get AquaScan, so
NextExpress users should get the same — **with NextScan branding**: the
three strings carrying the AquaScan name/credit are replaced (listing
banner, help banner incl. `Copyright © 1994 Aquarius`, help line
`F W - Configure AquaScan`); frame widths preserved by flexing the dash
runs; every other byte matches the captures. Stock internal behaviour is
documented below purely as the difference record. The stock captures
were taken with the three icons temporarily moved to
`BBS:Storage/DisabledCmds/` (restart required — the disk-object cache
keeps serving moved icons); the board was restored to as-shipped state
afterwards (all seven icons verified back).

## Observed: AquaScan (the parity target)

From `ae_tierd_aquascan3.txt` unless noted.

### Banner and scan headers

```
\x1b[0m\r\n\x1b[0m\x1b[34m--[ \x1b[36mAquaScan v1.0 by Aquarius/Outlaws \x1b[34m]---------------[ \x1b[36m'f ?' for options \x1b[34m]--\x1b[0m\r\n
```

The right-hand label varies with the invoking command and the dash run
flexes to keep the frame width: `'f ?'` (15 dashes), `'fr ?'` (14),
`'scan ?'` (12), `'nsu ?'` (13). Scan headers (plain text, no ANSI):

```
Scanning dir 1 from top... Ok!          (F 1; F U resolves to the last dir: "Scanning dir 2 from top")
Scanning dir 1 from top... Nothing found!   (empty Conf01/Dir1)
Scanning HOLD dir from top... Nothing found!  (F H)
Reverse-scanning dir 1... Ok!           (FR 1 — files emitted newest-first)
Scanning dir 1 for 06-10-26... Nothing found!  (SCAN/NSU date scans)
```

Bare `FR` skips the directories prompt and starts at the highest dir
(`Reverse-scanning dir 2... Ok!` observed, then descends).

### Per-file frame (the listing body)

Each parseable DIR row is re-rendered:

```
\x1b[0m                                            _\xb8,\xf8*\xa4\xb0\xac\xb0\xa4*\xf8,\xb8_\xb8,\xf8*\xa4\xb0\xac\xac\xb0\xa4*\xf8,\xb8_\r\n
\x1b[0m      \xb8,\xf8*\xa4\xb0\xac\xaf\xac\xb0\xa4*\xf8,\xb8_\xb8,\xf8*\xa4\xb0\xac\xb0\xa4*\xf8, 01-15-26\r\n
\x1b[0m\r\n
\x1b[0m\x1b[34m[\x1b[0m File #1 \x1b[34m]                    - ---- ---------------------------------- ---- -\r\n
\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34mP\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the\r\n
\x1b[0m\x1b[0m                                 Mirage art crew, January release.\r\n
```

- Separator art pair: second line carries the **file's date**.
- `[ File #N ]` header: the filler dashes shrink by one when N reaches
  two digits (`#10 ]` has 19 spaces vs 20 — width-stable).
- Row colouring: cyan filename (to col 13), blue status char, green
  size, yellow date, reset before the two spaces + description.
  Continuation lines emit plain at col 33.
- Rows the parser can't frame stream **plain, no colours, no frame**:
  observed for `THIRTEENCH.LZ` (13-char name, no status char) and
  `ALONGFILENAME.LHA` (shifted columns). A `P`-statused `F`-statused row
  both frame fine (`BADUPLD.LHA  F` got a frame).
- Footer: `\x1b[0;34m[\x1b[36m End of File List \x1b[34m]\x1b[0m\r\n`.

### Pager (`More?`) — single-key hotkeys

```
\x1b[0;36mMore? \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mC\x1b[32m)\x1b[36mlear, \x1b[32m(\x1b[33mF\x1b[32m/\x1b[33mR\x1b[32m)\x1b[36m Flag, \x1b[32m(\x1b[33m?\x1b[32m)\x1b[36m Help, \x1b[32m(\x1b[33mQ\x1b[32m)\x1b[36muit:\x1b[0m
```

- `Y` (and unrecognised keys) continue. `Q` echoes `Quit` and exits to
  the menu (two blank lines, then the menu prompt).
- `F` erases the prompt line and opens a line-read
  `\x1b[36mFile name(s) to flag:\x1b[0m ` — after the filename + CR it
  erases and returns **straight to `More?` with no confirmation**
  (flagging is silent).
- `ns` opens `\x1b[36mNon-stop scrolling! Are you sure \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[36m? ` —
  `Y` streams the rest without pausing.
- At the **post-`End of File List`** `More?`, `n` is rejected
  (backspaced out: `\x08 \x08`) and `Q` exits. Mid-list `n` semantics
  were not isolated cleanly (assumed stop-listing; verify when the Rust
  pager lands).

### Bare `F` — AquaScan's own directories prompt

```
\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m
```

Line-read (needs CR). Enter alone aborts silently to the menu. Invalid
input (e.g. a stray `F 1` line) → `Error in input!` and exit to menu.
Note the differences from the stock prompt: capital `N` in `None`,
space before `?`, no `(Enter)=none? ` parenthesis styling differences —
see the diff table.

### Help (`F ?`)

Preceded by a form-feed `\x0c`. Banner variant:

```
--[ AquaScan v1.0 by Aquarius/Outlaws ]---------[ Copyright \xa9 1994 Aquarius ]--
```

Body (plain + cyan, verbatim in `ae_tierd_aquascan3.txt` S1):

```
  F                           - Show the FileHelp and prompt for date and dir
  F R                         - Same as above but use reverse scanning
  F ?                         - Show this help text
  F W                         - Configure AquaScan
  F [R] dir [Q] [NS]          - Start scanning immediately
     ^  ^    ^   ^            (dir = U upload / A all / x number / H hold;
     ...                       Q = quick scan, first description line only;
                               NS = non-stop scrolling)
```

### `N` via the door (new files)

Opens `Date: (MM-DD-YY), (-X) Days, (R)everse, (Enter)…` (full ANSI in
`ae_tierd_aquascan3.txt` S12); bad input → `Error in date!`.

## Observed: stock internal `F`/`FR`/`N` (the difference record)

From `ae_tierd_stock.txt` F1 (canonical listing) and
`ae_tierd_stock2.txt` T1–T14. Captured with the door icons moved aside.

### Listing = raw DIR stream

```
F 1\r\n\r\nScanning directory 1\r\nANSIPACK.LHA P 234567  01-15-26  Collection of 40 ANSI screens from the\n\r                                 Mirage art crew, January release.\n\r...
```

- Header `Scanning directory N` (`express.e:27683`); HOLD variant
  `Scanning directory HOLD` (`:27688`); reverse header
  `Reverse scanning directory N` (`:27667`).
- The DIR file bytes stream **verbatim** — no colour, no frames, our
  edge-case rows appear exactly as authored (`MEGADEMO.DMS P12345678`
  with the unpadded size jammed against the status char).
- **Line-ending quirk**: rows arrive as `…the\n\r…` — LF then CR.
  `displayIt2` (`express.e:27738-27744`) emits the file's `\n` and then
  appends `\b` (CR). Inverted CRLF on the wire; terminals tolerate it.
  NextExpress does NOT reproduce this (we target AquaScan's `\r\n`).

### Stock pause prompt (the lineCount pager)

```
\x1b[32m(\x1b[33mPause\x1b[32m)\x1b[34m...\x1b[32m(\x1b[33mf\x1b[32m)\x1b[36mlags, More\x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m/\x1b[33mns\x1b[32m)\x1b[0m?
```

Unknown input is **rejected and redrawn** (`\x1b[A\x1b[K` + re-prompt) —
this ate the first stock battery's queued commands. Has an `(f)lags`
option (stock flagging integration; a bare `f` keypress redrew the
prompt in T4 — the exact accepted grammar wasn't isolated).

### Stock bare `F` directories prompt (getDirSpan, `express.e:26864`)

```
\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=none? \x1b[0m
```

(The `H` token only renders with hold access, `:26866` otherwise.)
Out-of-range → `\r\nNo such directory.\r\n\r\n` (`:26905`).

### Stock internal `N`

`Date as (mm-dd-yy) to search from (Enter)=: ` — and it **loops on
invalid input** (Enter alone re-prompts). This consumed the rest of the
stock2 session including the `G Y` (harness hazard — see below).
NextExpress's `N` targets the AquaScan date prompt instead.

## Diff summary — stock internal vs AquaScan (the experience we ship)

| Aspect | Stock internal | AquaScan (target) |
| --- | --- | --- |
| Listing source | streams `DIR<n>` file bytes verbatim | re-renders parsed rows at runtime |
| Banner | none | `--[ AquaScan v1.0 … ]--[ 'f ?' … ]--` (NextScan-branded in our port) |
| Scan header | `Scanning directory N` | `Scanning dir N from top... Ok!/Nothing found!` |
| Row presentation | raw, monochrome | per-file frame, separator art with date, colour-coded fields; unparseable rows fall through plain |
| Footer | none | `[ End of File List ]` |
| Pager | `(Pause)...(f)lags, More(Y/n/ns)? `, rejects+redraws unknown keys | `More? (Y/n/ns), (C)lear, (F/R) Flag, (?) Help, (Q)uit:`, unknown keys continue |
| Flagging | `(f)lags` inside pause prompt | `F`/`R` hotkeys → silent `File name(s) to flag:` |
| Bare-command prompt | `…(Enter)=none? ` (lowercase) | `…(Enter)=None ? ` (capital N, space before ?) + `Error in input!` |
| Reverse header | `Reverse scanning directory N` | `Reverse-scanning dir N... Ok!` |
| `N` (new files) | `Date as (mm-dd-yy) to search from (Enter)=: ` (loops on bad input) | `Date: (MM-DD-YY), (-X) Days, (R)everse, (Enter)…` + `Error in date!` |
| Line endings | LF-CR (inverted) | CR-LF |
| Non-stop | `NS` arg / `ns` at pause | `NS` arg / `ns` at More? + `Are you sure (Y/n)?` confirm |

## Transcript-reading hazards

1. **Door pagers eat scripted lines.** Any battery written for menu-at-
   a-time driving collides with hotkey pagers: in
   `ae_tierd_aquascan_accidental.txt` everything after F1, and in
   `ae_tierd_aquascan.txt` blocks A2/A4/A6/A9/A10, the SENT line was
   consumed by a pending `More?`/flag/confirm prompt — read the actual
   echoes, not the block labels.
2. **Internal `N`'s date prompt loops**: `ae_tierd_stock2.txt` T15
   onward (E1–E3, the final `J 2`, and the `G Y`) all fed the date
   prompt. The session never logged off; the board was restarted
   immediately after (node-spin hazard).
3. **`G Y` swallowed by a pager** (accidental pass) leaves a phantom
   login AND a spinning node — same mitigation, restart.
4. Stock-capture gap: the **stock empty-dir `F`** capture was lost to
   hazard 2 (expected output is just `Scanning directory 1` + nothing,
   per `displayIt` streaming an empty file — inference, not capture).
   AquaScan's empty-dir behaviour IS captured (`Nothing found!`).

## Open questions for the port

- Mid-list `n` at AquaScan's `More?` — assumed "stop listing", not
  cleanly isolated. Pin when the Rust pager's tests land, or capture
  opportunistically in a future session.
- AquaScan `Q`uick-scan token (`F 1 Q`) and `F W` configuration were
  not captured (W is out of scope — sysop-side config of the door).
- Whether AquaScan's date-bearing separator art varies for same-date
  consecutive files (it repeats the date per file in all captures).
