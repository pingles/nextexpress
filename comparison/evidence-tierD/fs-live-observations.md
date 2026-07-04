# Live observations — Slice D8 `FS` (file status), AmiExpress 5.6.0 reference

Captured **2026-07-04** against the genuine AmiExpress 5.6.0 binary
(FS-UAE Docker harness, container `nextexpress-ref-d8fs`, telnet
`127.0.0.1:27227`). One telnet session, one node grabbed, clean `G Y`
logoff (`** AutoSaving File Flags **` → `Click...` carrier drop → EOF
drain). Well under the §10.8 connection budget (1 open).

- Driver: [`comparison/harness/ae_tierd_fs.py`](../harness/ae_tierd_fs.py)
- Transcript: [`comparison/transcripts/ae_tierd_fs.txt`](../transcripts/ae_tierd_fs.txt)
- Fixture / account: seeded **sysop / sysop**, security level **255**,
  auto-rejoins conference 2 ("Amiga"). Board has **two** conferences:
  1 = "New Users", 2 = "Amiga". Both accessible to sysop.
- **NOT door-shadowed.** `FS` has no AquaScan icon
  (`slices/cmds-files-list.md:43`); the token dispatches straight to
  `internalCommandFS()` (`express.e:24872`). No §10.3 door-vs-source
  authority decision applies.

---

## HEADLINE FINDING — `FS` **denies** for the seeded sysop (live ground truth inverts the task assumption)

The task brief assumed "sysop sec 255" would pass the accounting gate and
render the `fileStatus(0)` table, and that the **deny path was
UNTESTABLE**. **The live board says the opposite.** Every `FS` variant
returns the `higherAccess()` message:

```
FS\r\n
\r\n
Command requires higher access.\r\n
\r\n
<menu prompt>
```

Repr (verbatim, transcript block "D8: FS (bare, from conf 2)"):

```
b'FS\r\n\r\nCommand requires higher access.\r\n\r\n\x1b[0m\x1b[35mNextExpress Reference \x1b[0m[\x1b[36m2\x1b[34m:\x1b[36mAmiga\x1b[0m] Menu (\x1b[33m597\x1b[0m mins. left): '
```

### Why (source-confirmed)

`internalCommandFS()` gates on `ACS_CONFERENCE_ACCOUNTING` **before**
calling `fileStatus`:

```
PROC internalCommandFS()
  IF checkSecurity(ACS_CONFERENCE_ACCOUNTING)=FALSE THEN RETURN RESULT_NOT_ALLOWED
  fileStatus(0)
ENDPROC RESULT_SUCCESS          -- express.e:24872-24875
```

`RESULT_NOT_ALLOWED` is rendered by `higherAccess()`:
`aePuts('\b\nCommand requires higher access.\b\n')` (`express.e:3037-3039`;
`\b\n` → `\r\n` on the wire).

`checkSecurity(ACS_CONFERENCE_ACCOUNTING)` (`express.e:8455-8497`) does
**not** derive from the numeric security level (255). It resolves through,
in order: the per-user `securityFlags` override string (default `?` =
unset) → not one of the special-cased flags → `TOOLTYPE_DEFAULT_ACCESS`
config → the per-level `ACCESS.<level>` tooltype file
(`checkToolTypeExists(TOOLTYPE_ACCESS,acsLevel,'CONFERENCE_ACCOUNTING')`).
On **this seeded board**, neither the default-access config nor the
`ACCESS.255` config lists `CONFERENCE_ACCOUNTING`, so the flag is FALSE
even for the sysop → `FS` denies. **Sec 255 ≠ accounting access.**

This is a genuine *configured-access* fact of the fixture, not a transient
and not a code bug — it reproduces across all four `FS` variants and both
conferences (see edge battery below).

### The accounting TABLE was still observed — via the LOGIN stats screen (`fileStatus(1)`)

The post-login user-statistics screen calls `fileStatus(1)`
(`express.e:25604`), which shares the same format strings as `fileStatus(0)`
but with `opt=1` (current-conf only) and a per-row gate that prints the
current conf **regardless of accounting access**
(`IF (ca=TRUE) OR (i=currentConf)`, `express.e:24168`). So the login screen
rendered the real header / rule / row bytes even though `FS` itself denies.
This is the byte-source for the shared-format answers to Q1–Q4 below —
**tagged as the opt=1 login variant, NOT `FS`'s opt=0 output.**

Captured table (transcript block "password -> POST-LOGIN", lines 63-68),
verbatim repr:

```
\x1b[32m              Uploads                 Downloads\r\n
\r\n
\x1b[32m    Conf  Files    Bytes          Files    Bytes          Bytes Avail  Ratio\r\n
\x1b[0m    ----  -------  -------------- -------  -------------- -----------  -----\r\n
\x1b[33m       2\x1b[0m> \x1b[33m      0               0       0               0    Infinite  \x1b[31mDSBLD\r\n
\x1b[0m\r\n
```

Byte-for-byte (single Python repr, whitespace preserved per the
vacuous-literal-pin lesson):

```
b'\x1b[32m              Uploads                 Downloads\r\n\r\n\x1b[32m    Conf  Files    Bytes          Files    Bytes          Bytes Avail  Ratio\r\n\x1b[0m    ----  -------  -------------- -------  -------------- -----------  -----\r\n\x1b[33m       2\x1b[0m> \x1b[33m      0               0       0               0    Infinite  \x1b[31mDSBLD\r\n\x1b[0m\r\n'
```

---

## The 5 critical answers

Answered from the observed `fileStatus(1)` login render (shared format
strings, `express.e:24153-24188`). **The `FS`-command (opt=0) success
table was NOT reachable with this fixture — see the headline finding.**

| # | Question | Answer | Source / tag |
|---|---|---|---|
| 1 | Byte/file columns — zeros or real stats? | **All ZERO.** Row: uploads `      0` files / `             0` bytes, downloads `      0` files / `             0` bytes. Pristine account, no accumulated U/D history on the volume. | captured (login row); **volatile-runtime** |
| 2 | Bytes-Avail — `Infinite` or a number? | **`Infinite`** — `loggedOnUser.todaysBytesLimit==0` (`express.e:24172-24176`). | captured; label stable-const, selection volatile-runtime |
| 3 | Ratio — `n:1` or red `DSBLD`? | **red `DSBLD`** — `loggedOnUser.secLibrary==0` takes the `[31mDSBLD` arm (`express.e:24181`). | captured; **volatile-runtime** (which arm depends on secLibrary) |
| 4 | Header variant — `Bytes` or `KBytes`? | **`Bytes`** (both column heads + the "Bytes Avail" label). The board's `CREDITBYKB` toggle is **OFF** (`express.e:24156-24160`). | captured; **stable-const** (fixed by the board toggle) — pin the `Bytes` header |
| 5 | How many conference rows, which confs? current-conf colour? | **Login screen (opt=1) showed exactly 1 row: conf `2` (current), coloured `n=3` → `[33m`.** The multi-conf row set of `FS`'s **opt=0** loop was **NOT observed** (FS denied). Extrapolated below. | Q5 partly captured (opt=1), opt=0 row set **extrapolated-from-source** |

### Current-conf colour cross-check (Q5)

`fileStatus` colours the current conf `n=3` and others `n=6`
(`express.e:24166`), applied as `[3\dm` → `ESC[33m` (current, yellow) /
`ESC[36m` (other, cyan). The login row for conf 2 (current) rendered
`\x1b[33m` — confirming `n=3` for the current conf. I could **not** observe
the `n=6` other-conf colour because (a) `FS` denies and (b) the login
`fileStatus(1)` only ever prints the current conf. **Extrapolated:** in a
successful `FS` (opt=0), the current conf renders `ESC[33m`, every other
accessible conf renders `ESC[36m`.

---

## Per-field stable/volatile tags (§10.6)

Stage 4 byte-pins only the stable rows; the volatile ones assert derivation.

| Field | Exact bytes (Latin-1 repr) | Tag |
|---|---|---|
| Leading blank | `\r\n` | stable-const |
| Section head | `\x1b[32m              Uploads                 Downloads\r\n` (ESC[32m, 14 SP, `Uploads`, 17 SP, `Downloads`) | **stable-const** |
| Blank | `\r\n` | stable-const |
| Column header (`Bytes` variant) | `\x1b[32m    Conf  Files    Bytes          Files    Bytes          Bytes Avail  Ratio\r\n` | **stable-const** (pin; the `Bytes`/`KBytes` choice is fixed by the board `CREDITBYKB=off` toggle) |
| Rule line | `\x1b[0m    ----  -------  -------------- -------  -------------- -----------  -----\r\n` | **stable-const** (pin; fixed dash geometry) |
| Row: rel-conf number | `\x1b[33m       2\x1b[0m` (`\r\d[4]` right-just width 4, preceded by 4 SP) | number **volatile-runtime** (which confs exist / are accessible); field width + `> ` suffix stable-const |
| Row: current/other colour | `[33m` (n=3 current) / `[36m` (n=6 other, extrapolated) | derivation **volatile-runtime** (tracks currentConf) |
| Row: upload files | `      0` (`\d[7]`) | **volatile-runtime** (`loggedOnUser.uploads AND $FFFF`) |
| Row: upload bytes | `             0` (`\r\s[14]`, `formatBCD`) | **volatile-runtime** (`uploadBytesBCD`) |
| Row: download files | `      0` (`\r\d[7]`) | **volatile-runtime** (`loggedOnUser.downloads AND $FFFF`) |
| Row: download bytes | `             0` (`\r\s[14]`, `formatBCD`) | **volatile-runtime** (`downloadBytesBCD`) |
| Row: Bytes-Avail | `    Infinite` (`\r\s[9]`) | value **volatile-runtime** (`Infinite` when `todaysBytesLimit==0`, else `formatUnsignedLong(todaysBytesLimit-dailyBytesDld)`); the word `Infinite` is a stable-const literal |
| Row: Ratio | `  \x1b[31mDSBLD` (secLibrary==0 arm) OR `  <n>[0m:[3<n>m1` (secLibrary≠0 arm, `express.e:24179`) | **volatile-runtime** (arm + ratio value depend on `secLibrary`); the `DSBLD` literal and `:1` suffix are stable-const |
| Trailer | `\x1b[0m\r\n` | stable-const |

**Encoding (§10.7):** the `FS` table and deny message are **pure ASCII**
(every byte < 0x80) — no re-encode needed, no mojibake risk. The only
high-bit byte in the whole session is `0xA9` (©) in the connect banner
`Copyright ⟨a9⟩2018-2023 Darren Coles` — Latin-1 `0xA9` → target UTF-8
`U+00A9` (`\xc2\xa9`). Not part of the `FS` surface; noted for completeness.

**Interactive surfaces (§10.7):** **NONE.** `fileStatus` opens **no
sub-prompt, no pager, no hotkey loop** — it prints the table and returns
to the menu in one shot. The Stage-5 per-keystroke human-glance prompt
does **not** need to fire for `FS`. (The deny path is likewise a single
line, no prompt.)

---

## Edge-probe battery (`edge-probe-battery.md`)

| Row | Probe | Sent | Result | Captured / extrapolated |
|---|---|---|---|---|
| 13 | case-fold | `fs` (lowercase) | Identical to `FS` — `Command requires higher access.` Token dispatch is case-insensitive; the gate denies before any output difference. | **captured** |
| 7 | trailing numeric arg | `FS 1` | Identical deny. `internalCommandFS()` takes **no params** — the tail is never parsed; the ACS gate denies first. | **captured** |
| 14 | junk inline arg | `FS xyz` | Identical deny. Same reason — tail ignored, gate denies. | **captured** |
| 10 | empty-collection / single-conf | (row-set count) | Login `fileStatus(1)` showed 1 row (current conf). opt=0 multi-conf loop unobserved (FS denied). See extrapolation. | partial capture + extrapolated |
| — | current-conf colour | `FS` from conf 1 vs conf 2 | Both denied identically (menu prompt just reflects the joined conf). Colour cross-check for the *table* unobservable because FS denies. | captured (deny); table colour extrapolated |
| — | **deny path** | any `FS` as seeded sysop | **`\r\nCommand requires higher access.\r\n`** — `higherAccess()`, `express.e:3038`, reached via `RESULT_NOT_ALLOWED` at `express.e:24873`. | **CAPTURED** (task expected this untestable — it is the live default) |

### Extrapolated-from-source — the opt=0 success table (UNCAPTURABLE with this fixture)

To reach `fileStatus(0)` the account must pass
`checkSecurity(ACS_CONFERENCE_ACCOUNTING)` — which this fixture's sysop
does **not**. So the multi-conf table is structurally uncapturable here.
From `express.e:24141-24190`, a **successful** `FS` (opt=0) would emit:

- the same section head / `Bytes` header / rule lines (shared format
  strings — byte-pinned above from the login render);
- then `FOR i:=1 TO cmds.numConf` (`=2` on this board), each row gated by
  `checkConfAccess(i)` (both confs accessible to sysop) **and**
  `(ca OR i=currentConf)` (with accounting access, `ca=TRUE`, so all
  accessible confs print):
  - conf **1** ("New Users") → colour `n=6` → `ESC[36m`, rel-conf `       1`;
  - conf **2** ("Amiga", current) → colour `n=3` → `ESC[33m`, rel-conf `       2`;
- each row's U/D counters, Bytes-Avail and Ratio populated as in the
  captured login row (all-zero / `Infinite` / `DSBLD` for the pristine
  sysop — the same runtime-derived fields, same volatility);
- `saveMsgPointers`/`loadMsgPointers` bracket the loop (`:24162`,`:24189`)
  and per-row `loadMsgPointers(i,1)` when `ca` (`:24169`) — no wire output;
- trailing `\x1b[0m\r\n` (`:24188`).

**Tag: extrapolated-from-source (`express.e:24141-24190`).** Two-conf,
two-row table; row set = every `checkConfAccess`-passing conference. NOT
invented from a wire fragment — the header/rule/row *format* is pinned
from the live login capture; only the multi-conf **loop count** and the
`n=6` other-conf colour are source-derived.

---

## Scope-fork note for Stage 3 (parity-target decision required)

The reference board, **as seeded**, does not grant the sysop
`ACS_CONFERENCE_ACCOUNTING`, so **`FS` denies**. Stage 3 must decide the
NextExpress parity target explicitly:

1. **Match the board as-shipped** → NextExpress's seeded sysop also lacks
   accounting access → `FS` → `Command requires higher access.` (the
   captured, byte-pinnable deny). The accounting table then only ever
   appears on the login stats screen (`fileStatus(1)`), not from `FS`.
2. **Grant accounting access** to the NextExpress sysop → `FS` renders the
   opt=0 table (extrapolated shape above), and the reference capture of
   that table would require modifying the fixture's `ACCESS.255` /
   default-access config and recycling the container (a fixture change,
   out of Stage-2 scope, not performed here).

Either way, the header/rule/row **format** bytes are grounded (login
capture) and the deny **message** bytes are grounded (FS capture). What is
*not* grounded is a live opt=0 multi-conf table — that is extrapolated
until/unless the fixture grants accounting access.
