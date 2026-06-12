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
| `comparison/transcripts/ae_tierd_aquascan4.txt` | Recapture of the six unverified pager corners (U1–U7), all pinned — three corrected the design's provisionals |
| `comparison/transcripts/ae_tierd_aquascan5.txt` | `F A` across an emptied Dir1 (V1) + `F 1` on the empty dir (V2); Dir1 restored from fixtures afterwards |
| `comparison/transcripts/ae_tierd_probes.txt` | **Byte-at-a-time probe battery** (P1 held-`n`+CR, P2 bare LF, P3 per-byte flag echo) — per-step `<<<sent … got>>>` idle snapshots expose echo timing; the final `Q` recovery sits under a mislabelled `PARTIAL (pre-failure)` block (script wart; the run succeeded), and P2's recovery bytes were consumed unrecorded |

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
- **`n` is a buffered prefix, not a stop verb** (aquascan4.txt U1 +
  aquascan3.txt S2 — mid-list and post-End behave identically): the
  door echoes `n` and waits, because `n` is ambiguous between
  `N` (= Quit, per the in-pager help) and `ns`; the next key emits
  `\x08 \x08` (erasing the n) and then runs its own verb.
- `F` erases the prompt line and opens a line-read
  `\x1b[36mFile name(s) to flag:\x1b[0m ` — after the filename + CR it
  erases and returns **straight to `More?` with no confirmation**
  (flagging is silent). `R` is the same with the distinct prompt
  `File number(s) to flag:`.
- `ns` opens `\x1b[36mNon-stop scrolling! Are you sure \x1b[32m(\x1b[33mY\x1b[32m/\x1b[33mn\x1b[32m)\x1b[36m? ` —
  `Y` streams the rest without pausing; declining with `n` (unechoed)
  clears and redraws `More?`, paged mode continuing (aquascan4.txt U3).
- `?` shows the **in-pager pause help** (aquascan4.txt U2), not the
  `F ?` screen: `\x0c\r\n` + `These are the commands that can be used
  at the pause prompt:` + a verb table revealing the full surface —
  `(Enter),(Y),(Space)` continue, `(C)` clear+continue,
  `(DownArrow),(3)` page down, `(UpArrow),(9)` page up, `(7)` start,
  `(5)` redraw, `(NS)` non-stop, `(?)` help, `(F)` flag by name,
  `(R),(#)` flag by number, `(K)` skip dir, `(L)` reload dir,
  `(N),(Q)` Quit, `(Ctrl-C)` quit anytime, `(D)` quit and download,
  `(X)` mark fake file, `(V)` view a file, `(O)` who online,
  `(Z)` zippy search, `(A)` alter file flags — then a
  `\x1b[0m~SP|\x0c\x1b[0m\r\n` marker and a full redraw of the current
  page from its first line, ending back at `More?`.

### Bare `F` — AquaScan's own directories prompt

```
\x1b[36mDirectories: \x1b[32m(\x1b[33m1-2\x1b[32m)\x1b[36m, \x1b[32m(\x1b[33mA\x1b[32m)\x1b[36mll, \x1b[32m(\x1b[33mU\x1b[32m)\x1b[36mpload, \x1b[32m(\x1b[33mH\x1b[32m)\x1b[36mold, \x1b[32m(\x1b[33mEnter\x1b[32m)\x1b[36m=None ?\x1b[0m
```

Line-read (needs CR). Enter alone aborts silently to the menu. Invalid
input (e.g. a stray `F 1` line) → `Error in input!` and exit to menu.
`A`/`U`/`H` answers behave exactly like the argument spans
(aquascan4.txt U5–U7; `U` → `Scanning dir 2 from top... Ok!`,
confirming upload = highest dir via the prompt path too).
Note the differences from the stock prompt: capital `N` in `None`,
space before `?` — see the diff table.

**Junk arguments at the menu** (`F XYZ`, aquascan4.txt U4) do NOT take
the `Error in input!` path: they emit the **help-banner variant** (the
`Copyright © 1994 Aquarius` banner, no form-feed) followed by
`Argument error! Type 'f ?' for help.` and a single-`\x1b[0m\r\n` exit
tail.

**Empty-dir transition in an `A` span** (aquascan5.txt V1): an empty
dir emits exactly its `Scanning dir 1 from top... Nothing found!` line
with the next dir's `Scanning dir 2 from top... Ok!` directly on the
next line — no blank between, no `More?`, one banner for the whole
span; the blank line comes after the last scan header, before the
first frame.

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

(The 2026-06-10 recapture session — aquascan4.txt, aquascan5.txt —
pinned the original six: mid-list `n`, `?` at More?, ns-confirm
declined, junk menu args, A/U/H prompt answers, empty-dir `F A`
transition. Folded into the sections above.)

- AquaScan `Q`uick-scan token (`F 1 Q`) and `F W` configuration were
  not captured (W is out of scope — sysop-side config of the door);
  whether the door accepts `F R 1` rather than Argument-erroring is
  also uncaptured — worth pinning before slice D3 flips `F R`.
- The in-pager help advertises navigation verbs with uncaptured
  behaviours (DownArrow/3 page down, UpArrow/9 page up, 7 start,
  5 redraw, K skip dir, L reload dir) and cross-tier verbs (D, X, V,
  O, Z, A). NextScan D2 ships the help text verbatim but treats them
  as advertised-but-inert (unknown keys continue); each is owed to its
  owning slice.
- The `~SP|` + form-feed marker emitted between the in-pager help and
  the page redraw (aquascan4.txt U2) — origin unclear; reproduce
  byte-for-byte.
- Whether AquaScan's date-bearing separator art varies for same-date
  consecutive files (it repeats the date per file in all captures).

## Probe battery 2026-06-12 — the three uncaptured interaction corners

Run via `comparison/harness/ae_tierd_probes.py` →
`comparison/transcripts/ae_tierd_probes.txt`. Single session, each probe
byte sent alone with a per-step idle snapshot (the `<<<sent … got>>>`
markers inline in the stream) — built specifically to expose the echo
timing that flat captures hide. Verdicts feed design
`2026-06-12-utf8-hotkeys-flagmark-design.md` §4 (slice D2b).

### P1 — held lone `n`, then bare CR: quits to the menu (probes.txt:100-138)

At More?, the `n` echoed back alone within its 3s snapshot window and the
door held — `<<<sent b'n' got>>>n` — nothing else followed. The follow-up
CR went straight to the menu:

```
<<<sent b'\r' got>>>\r\n\x1b[0m\r\n\x1b[0m\r\n\x1b[0m\x1b[35mNextExpress Reference \x1b[0m[\x1b[36m2\x1b[34m:\x1b[36mAmiga\x1b[0m] Menu (\x1b[33m599\x1b[0m mins. left):
```

So `n`+Enter = Quit, as the in-pager help promised — but the wire shape is
NOT `Q`'s: **no `\x08 \x08` erase of the held `n`, and no `Quit` echo**.
The same session's `Q` keypress (the mislabelled PARTIAL block, :217/:222)
shows `Quit\r\n\x1b[0m\r\n\x1b[0m\r\n` + menu; the n+CR exit is
byte-identical except the `Quit` word is replaced by the echoed `\r\n`
(the held `n` stays on the prompt line). Design §4 amended to pin this.

### P2 — bare LF at a fresh More?: swallowed entirely (probes.txt:140-175)

```
<<<sent b'\n' got>>><<<end P2 bare LF>>>
```

Zero bytes back in a 5-second window. No echo, no page stream (a continue
would have streamed File #4 onward — the dir holds 28), no Quit/menu. So
LF is not Enter, and not even an unknown-key continue: the input layer
drops it outright. This CONTRADICTS the design's provisional
"bare LF maps to Enter" rule — design §4 amended (bare LF → no key event).

### P3 — flag prompt echoes per keystroke (probes.txt:177-212)

`F` at More? wiped the prompt (`\r` + 69sp + `\r`) and opened
`File name(s) to flag: `. Every single byte sent came back alone within
its 2-second window:

```
<<<sent b'T' got>>>T<<<end P3 byte T>>><<<sent b'E' got>>>E<<<end P3 byte E>>><<<sent b'R' got>>>R<<<end P3 byte R>>><<<sent b'M' got>>>M<<<end P3 byte M>>><<<sent b'V' got>>>V<<<end P3 byte V>>><<<sent b'4' got>>>4<<<end P3 byte 4>>><<<sent b'8' got>>>8<<<end P3 byte 8>>>
```

Per-keystroke echo at the door's line prompt is proven. The finishing
`.LHA\r` echoed `.LHA` then `\r` + 79sp + `\r` + the More? redraw — the
already-captured silent-flag wipe shape, now with its echo timing pinned.

### Side effects observed (the frame-effects hazard made real)

This session's logon greeted with `** Flagged File(s) Exist **\r\n\x07`
(:77-78) — flags left by earlier capture sessions persist on the board —
and the post-P3 logoff emitted
`** AutoSaving File Flags **\r\n\x07\r\nClick...` (:224-229): P3's flag of
TERMV48.LHA was genuinely recorded and saved. The board now carries that
flag for sysop; future capture sessions will see the logon banner.

## Methodology blind spots

What flat transcript captures structurally cannot show (each bit us once):

1. **Echo timing at prompts.** A flat capture of `n` + CR contains the same
   bytes whether the door echoes per keystroke or echoed the whole line
   after Enter — interaction granularity (hotkey vs line read, immediate vs
   deferred echo) is invisible unless each byte is sent alone and the wire
   snapshotted before the next byte (this battery's `<<<sent … got>>>`
   shape). The D2 dead-prompt defect shipped exactly through this gap.
2. **Charset/encoding context of high-bit bytes.** Captures record `\xb8`,
   `\xf8`, `\xa9` … but not which glyph the sender meant — the Latin-1/
   Topaz intent is an out-of-band fact about the platform, so byte-faithful
   replay onto a UTF-8 wire produced mojibake (the D2u defect). A capture
   can never arbitrate encoding; that is a policy decision to document.
3. **Effects outside the scenario frame.** A capture frames one command's
   bytes; consequences that surface elsewhere — the silent flag's
   `** Flagged File(s) Exist **` at next logon, the
   `** AutoSaving File Flags **` logoff banner, ratio/state changes — are
   absent unless a scenario deliberately spans the frame. The probe
   session's own logon and logoff (above) caught both flag surfaces,
   proving the "silent" flag prompt was never a no-op.
