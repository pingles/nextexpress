# Live observations — AmiExpress 5.6.0 reference, Tier C commands

Captured 2026-06-10 against the genuine AmiExpress 5.6.0 binary
(FS-UAE Docker harness, `docker/amiexpress-fsuae`, two conferences seeded:
1 "New Users", 2 "Amiga", each with a single message base; sysop account
has access to both; `QUIET_JOIN` not set; no `JoinConf` / `JoinMsgBase`
screen files installed, so nothing renders before the prompts).

Raw transcripts: `comparison/transcripts/ae_tierc.txt`,
`ae_tierc2.txt`, `ae_tierc3.txt` (every block has RENDER + Python-repr
byte forms). Cross-checked against `amiexpress/express.e` — control flow
quoted in `legacy-J-noarg.md`, `legacy-prevnext.md`, `legacy-JM.md`
(this directory); I verified the four `internalCommandLT/GT/LT2/GT2`
procs, `internalCommandJ` (:25113-25183) and `internalCommandJM`
(:25185-25237) against the raw E source byte-for-byte.

## Transcript-reading hazards (do NOT trust raw labels blindly)

1. In `ae_tierc.txt`, the blocks labelled "C3: < from wherever" and
   "C3: < once more" sent `<` **into a pending `Conference Number (1-2): `
   prompt**, not at a menu prompt — they show prompt-input clamping
   (`Val("<")=0 → 1`), not the `<` command.
2. In `ae_tierc.txt`, the block "C4b: enter 1 at JM prompt" actually sent
   `1` at a **menu** prompt — `1` is the sysop account-editor command.
   The following "dotted: J 1.1" block fed text into that editor's
   prompt; the **clean** `J 1.1` capture is in `ae_tierc2.txt` (GAP2).
3. `joinConf` persists the conference you join as the auto-rejoin target
   (`express.e:5135`), so each session's login conference depends on the
   previous session's last join.

## Observed wire behaviour (clean captures, IAC-stripped)

### `J` with no / invalid / out-of-range argument → interactive prompt

`J`, `J 99`, `J 0`, `J abc`, `J -1` all behave identically at the menu:

```
b'J\r\nConference Number (1-2): '          (ae_tierc.txt; echo of typed line, then prompt)
b'J 99\r\nConference Number (1-2): '       (ae_tierc2.txt GAP3)
b'J 0\r\nConference Number (1-2): '        (ae_tierc2.txt GAP3b)
b'J abc\r\nConference Number (1-2): '      (ae_tierc2.txt GAP3c)
```

- **No** blank line between the echoed command and the prompt (the
  JoinConf screen file is absent; legacy renders it here when present,
  `express.e:25143`).
- Upper bound = total configured conference count (`cmds.numConf`).
- The prompt has a trailing space and **no trailing CRLF**.

### Prompt input semantics (single-shot, never re-prompts)

| Input at `Conference Number (1-2): ` | Observed result |
| --- | --- |
| blank (bare Enter) | `b'\r\n\r\n'` + menu prompt — silent abort, stays in current conference |
| `1` | joins conf 1 — `b'1\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m New Users\r\n…'` |
| `2` | joins conf 2 (ae_tierc2.txt GAP3c) |
| `99` | **clamped to 2** — joins Amiga (ae_tierc.txt) |
| `0` | **clamped to 1** — joins New Users |
| `abc` | `Val("abc")=0` → **clamped to 1** — joins New Users |

Source: clamp at `express.e:25153-25154`, after the single `lineInput`.
There is no re-prompt loop and no error message for non-numeric input.

### Join output from the prompt = join output of direct `J <n>`

```
b'1\r\n\r\n\x1b[32mJoining Conference\x1b[33m:\x1b[0m New Users\r\n\r\n
\x1b[32mTotal messages           \x1b[33m:\x1b[0m 0\r\n\r\n
\x1b[32mLast message auto scanned\x1b[33m:\x1b[0m 0\r\n
\x1b[32mLast message read        \x1b[33m:\x1b[0m 0\r\n\r\n\r\n'  + menu prompt
```

(The `Total messages` stats block is the legacy mail-stat display the
Rust port does not yet ship — its absence is an already-recorded
COMMAND_PARITY divergence, out of Tier C scope. The `Joining Conference`
line itself byte-matches the Rust `format_explicit_join_line`.)

### Dotted and two-token `J` arguments

| Command at menu | Observed |
| --- | --- |
| `J 1.1` | joins conf 1 (its only base) — normal join output (ae_tierc2.txt GAP2) |
| `J 2.1` | joins conf 2 — normal join output |
| `J 1 2` | `b'J 1 2\r\nMessage Base Number (1-1): '` — base 2 is out of range for single-base conf 1, so the **message-base prompt** fires (`express.e:25169-25180`); note it fires even on a single-base conference |

Source: `express.e:25130-25136` — first param `Val` = conference; text
after `.` = message base; else a second whitespace token = message base;
default base 1. After J's message-base prompt the input is passed to
`joinConf` **unclamped** — `joinConf` resets an out-of-range base to 1
(`express.e:4995`). (JM's own prompt instead clamps to `cnt`,
`express.e:25233-25234`.)

### `<` / `>` — prev/next conference

| State | Command | Observed |
| --- | --- | --- |
| conf 1 | `>` | joins conf 2 — normal join output (ae_tierc.txt) |
| conf 2 | `<` | joins conf 1 — normal join output (ae_tierc3.txt) |
| conf 2 (top) | `>` | `b'>\r\nConference Number (1-2): '` — falls into the J interactive prompt (ae_tierc2.txt GAP4) |
| conf 1 (bottom) | `<` | `b'<\r\nConference Number (1-2): '` — same fallback (ae_tierc2.txt GAP5) |
| at that prompt | blank | silent abort, stays put |

No wraparound. Walk skips inaccessible conferences (source; the seeded
sysop has access to both so skipping is not observable live). Success
always lands on **message base 1** (`joinConf(newConf,1,…)`,
`express.e:24543/:24562`).

### `<<` / `>>` / `JM` on a single-message-base conference

All of `>>`, `<<`, `JM 1`, `JM 9`, `JM abc`, `JM` (no arg) produce
exactly (ae_tierc.txt):

```
b'\r\nThis conference does not contain multiple message bases\r\n\r\n'
```

then a blank line + menu prompt (total `…\r\n\r\n\r\n` before the menu
prompt bytes). No join happens, no prompt is shown — even for the
"valid" `JM 1`. Source: the `NMSGBASES` tooltype probe at
`express.e:25211-25215` returns -1 when absent (the normal single-base
configuration), failing before any range logic.

### `JM` with a dotted argument

`JM 1.1` → joins conf 1 — **identical to `J 1.1`** (ae_tierc.txt;
delegation at `express.e:25203-25206` hands the raw params to
`internalCommandJ`).

### Multi-message-base behaviour (NOT observable on the reference)

The reference install has no multi-base conference, so the following is
pinned from source only (`legacy-JM.md`, `legacy-prevnext.md`):

- `JM <in-range n>` joins base n of the current conference.
- `JM` no-arg / out-of-range arg → JoinMsgBase screen (conf-dir, then
  node-dir fallback) + `Message Base Number (1-N): ` single-shot prompt;
  blank aborts silently; other input `Val` + clamp to [1,N] → join.
- `<<` / `>>` step `currentMsgBase ∓ 1`; past either end → the `JM`
  no-arg flow (which on a single-base conf prints the failure above).
- Join announcement gains the base name only when the conference has >1
  base: `\x1b[32mJoining Conference\x1b[33m:\x1b[0m <name> [<base>]`
  (`express.e:5077-5084` — spacing identical to the single-base form,
  with ` [<base>]` appended).

### `Val()` prefix semantics (pinned live, `ae_tierc4.txt`)

| Input | Observed |
| --- | --- |
| `J 2abc` (direct arg) | joins conf 2 — `Val` parses the leading digit run and stops at the first non-digit |
| `2abc` typed at the conference prompt | joins conf 2 — same prefix parse |
| `J +2` (direct arg) | **opens the prompt** — a leading `+` is NOT a valid sign for `Val` (yields 0) |
| `J -1` (direct arg) | opens the prompt (`Val("-1") = -1 < 1`; `-` IS a valid sign) |

So the Rust `val_prefix` helper must accept an optional leading `-` (only),
then a digit run, stopping at the first other character; no digits → 0.

### Command echo & terminal framing

The BBS echoes the typed line + CRLF (server-side echo); every
observation above starts with that echo. The menu prompt that follows
output blocks is the standard
`\x1b[0m\x1b[35m<bbs> \x1b[0m[\x1b[36m<n>\x1b[34m:\x1b[36m<name>\x1b[0m] Menu (\x1b[33m<m>\x1b[0m mins. left): `.
