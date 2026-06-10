# Legacy evidence: "J" (Join Conference) no-argument / invalid-argument flow

Source: `/Users/paul/Documents/GitHub/nextexpress/amiexpress/express.e` (plus `axenums.e`,
`axconsts.e`, `axcommon.e`). All line numbers refer to those files. `\b\n` in E string
literals is the AmiExpress end-of-line (CR LF on telnet). `[33m` etc. are literal ANSI
CSI sequences embedded in the source (ESC byte before `[`).

---

## 1. Dispatch (processInternalCommand)

- `processSysCommand` (express.e:28258) splits the typed line at the **first space**:
  `cmdcode` = text before the space, `cmdparams` = everything after (28265-28271). It tries
  SysCmd/BbsCmd doors first, then falls through to
  `processInternalCommand(cmdcode,cmdparams,TRUE)` (28283).
  (Note: the related `processCommand` path upper-cases `cmdcode` at 28245.)
- `PROC processInternalCommand(cmdcode,cmdparams,privcmd=FALSE)` — express.e:28285.
- The `J` entry (express.e:28318-28319):
  ```e
  ELSEIF (StrCmp(cmdcode,'J'))
    res:=internalCommandJ(cmdparams)
  ```
  `JM` is a separate token right after (28320-28321 → `internalCommandJM(cmdparams)`).
- Unknown command (privcmd=FALSE) prints (28397):
  `'\b\nNo such command!!  Use ''?'' for command list.\b\n\b\n'`
- If the handler returns `RESULT_NOT_ALLOWED` and `privcmd=FALSE`, `higherAccess()` runs
  (28400). `higherAccess` (3037-3039) prints:
  `'\b\nCommand requires higher access.\b\n'`

`RESULT_*` values (axenums.e:23):
`RESULT_FAILURE=-1, RESULT_SUCCESS=0, RESULT_NOT_ALLOWED=1, RESULT_ABORT=-2, RESULT_TIMEOUT=-3, RESULT_NO_CARRIER=-4, ...`

---

## 2. internalCommandJ — express.e:25113-25183

### Locals
`newStr[5]:STRING` (so the prompt input buffer holds max 5 chars), `newConf`, `stat`,
`newMsgBase=1` (default), `tempStr[255]`.

### Control flow (pseudocode with line refs)

```
internalCommandJ(params):                              # 25113
  IF checkSecurity(ACS_JOIN_CONFERENCE)=FALSE
      RETURN RESULT_NOT_ALLOWED                        # 25120  (caller prints "Command requires higher access.")
  saveMsgPointers(currentConf,currentMsgBase)          # 25121  persist read pointers of conf being left
  setEnvStat(ENV_JOIN)                                 # 25123  local node-status window only: "JOINING CONF" (24324-24326); nothing on the wire
  parseParams(params)                                  # 25125  split on spaces, items upper-cased & trimmed (8709-8745)

  newConf := -1                                        # 25127
  IF parsedParams.count() > 0                          # 25128
    param := item(0)                                   # 25129
    IF StrLen(param) > 0                               # 25130
      newConf := Val(param)                            # 25131  E built-in: parses leading number; 0 if non-numeric
      IF (pos := InStr(param,'.')) >= 0                # 25132  dotted form "J 2.3"
        newMsgBase := Val(param+pos+1)                 # 25133  text after the '.'
      ELSEIF parsedParams.count() > 1                  # 25134  two-arg form "J 2 3"
        newMsgBase := Val(item(1))                     # 25135
      ENDIF
  ENDIF

  newConf := getInverse(newConf)                       # 25140  relative→absolute mapping (see §5)

  IF (newConf < 1) OR (newConf > cmds.numConf)         # 25142  ── NO-ARG / INVALID-ARG branch ──
    displayScreen(SCREEN_JOINCONF)                     # 25143  node screen "JoinConf" (see §4)
    StringF(tempStr,'Conference Number (1-\d): ',cmds.numConf)   # 25144
    stat := lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)         # 25145  maxLen 5, timeout 300s
    IF stat <> RESULT_SUCCESS THEN RETURN stat         # 25146  timeout/abort/carrier-loss bubble up
    IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS     # 25148  blank input = silent abort, stay in current conf
    newConf := getInverse(Val(newStr))                 # 25150  asked ONCE — no re-prompt loop
  ENDIF

  IF newConf < 1 THEN newConf := 1                     # 25153  clamp low  (non-numeric input lands here: Val→0→getInverse→0→1)
  IF newConf > cmds.numConf THEN newConf := cmds.numConf  # 25154  clamp high

  IF checkConfAccess(newConf) = FALSE                  # 25156
    aePuts('\b\nYou do not have access to the requested conference\b\n\b\n')  # 25157
    RETURN RESULT_FAILURE                              # 25158
  ENDIF

  IF StrLen(getConfLocation(newConf)) = 0              # 25161  callerslog only, nothing to user:
    callersLog('****Conference Location unknown in MENU routines****')        # 25162
    callersLog('**** For Conference \d' formatted with newConf)               # 25163-25164
  ENDIF

  cnt := getConfMsgBaseCount(newConf)                  # 25167  NMSGBASES tooltype, default 1 (2048-2052)

  IF (newMsgBase < 1) OR (newMsgBase > cnt)            # 25169  only when user supplied an out-of-range msgbase
                                                       #        (default newMsgBase=1 never triggers it)
    IF displayScreen(SCREEN_CONF_JOINMSGBASE)=FALSE    # 25170  conf-dir "JoinMsgBase" first…
      displayScreen(SCREEN_JOINMSGBASE)                # 25171  …else node-dir "JoinMsgBase"
    ENDIF
    StringF(tempStr,'Message Base Number (1-\d): ',cnt)          # 25173
    stat := lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)         # 25174
    IF stat <> RESULT_SUCCESS THEN RETURN stat         # 25175
    IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS     # 25177  blank aborts the whole join
    newMsgBase := Val(newStr)                          # 25179  again asked ONCE; invalid value clamped later by joinConf (4995)
  ENDIF

  joinConf(newConf, newMsgBase, FALSE, FALSE)          # 25182  confScan=FALSE, auto=FALSE, forceMailScan=FORCE_MAILSCAN_NOFORCE
ENDPROC RESULT_SUCCESS                                 # 25183  joinConf's return value is discarded
```

### Key behavioural facts

1. **The prompt is shown exactly once.** There is no re-prompt loop. After the single
   `lineInput`, the value is clamped to `[1, cmds.numConf]` (25153-25154) and the flow
   proceeds. So:
   - `J` (no arg) → prompt. Enter `0`, `x`, or anything non-numeric → `Val`=0 →
     `getInverse(0)`=0 → clamped to **conference 1** (then access-checked).
   - Enter a number above range: with relative numbering OFF, `getInverse` is identity, so
     it clamps to `cmds.numConf` (highest conference). With relative numbering ON,
     `getInverse` returns 0 for a count past the accessible set (8573-8580) so it clamps to
     **1** instead. (Asymmetric — depends on the toggle, see §5.)
   - Blank input (just Enter) → `RETURN RESULT_SUCCESS` with **no message**; the user stays
     in the current conference (the only output is the `\b\n` that `lineInput` always emits
     at 2378).
2. **Prompt N is the absolute conference count** `cmds.numConf` (25144), even when
   relative conference numbering is enabled.
3. **No `+`/`-` relative-step parsing exists in `internalCommandJ`.** `J -1` parses as
   Val=-1 → getInverse→0 → the no-arg prompt branch. Previous/next navigation is provided
   by the separate `<` and `>` commands (internalCommandLT 24529-24546, internalCommandGT
   24548-24564), which themselves fall back to `internalCommandJ('')` when they walk off
   either end of the accessible conference list. (`<<`/`>>` do the same for message bases,
   falling back to `internalCommandJM('')`, 24566-24592.)
4. **Dotted form IS supported**: `J 2.3` → conference `Val("2.3")`=2 (Val stops at the
   `.`), message base `Val("3")`=3 (25132-25133). Space form `J 2 3` is equivalent
   (25134-25135). `JM` with a dotted argument delegates to `internalCommandJ(params)`
   (25203-25206).
5. The message-base prompt (25169-25180) fires **only** when an explicit msgbase argument
   was out of `1..cnt`; the plain no-arg `J` flow never shows it because `newMsgBase`
   defaults to 1. Note that at this point `confScreenDir` still belongs to the conference
   being *left* (joinConf has not yet run), so `SCREEN_CONF_JOINMSGBASE` resolves against
   the **current** conference's screens dir.
6. The access-denied string (25157) fires when `checkConfAccess(newConf)=FALSE` *after*
   clamping — i.e. the user explicitly named (or was clamped onto) a conference whose
   access flag is not set. The command then returns `RESULT_FAILURE` (no `higherAccess`
   text — that only triggers on `RESULT_NOT_ALLOWED`).

---

## 3. joinConf — express.e:4975-5139

### Signature (4975)

```e
PROC joinConf(conf, msgBaseNum, confScan, auto, forceMailScan=FORCE_MAILSCAN_NOFORCE)
```

- `conf`, `msgBaseNum` — absolute conference / message-base numbers.
- `confScan` — TRUE = pointer/scan pass only: skips setting `currentConf`/`currentMsgBase`
  (4997-5000) and skips the whole announcement/stats block (5056-5117) and the
  rejoin-persist block (5130-5138).
- `auto` — TRUE = auto-rejoin at logon: prints the "Auto-ReJoined" variant, runs
  `scanHoldDesc()` + `processSysCommand('S')` (5066-5068) and `displaySysopULStats()`
  (5115), and **skips the mail scan** (guard `auto=FALSE` at 5119).
- `forceMailScan` — enum (axenums.e:40)
  `FORCE_MAILSCAN_NOFORCE, FORCE_MAILSCAN_ALL, FORCE_MAILSCAN_SKIP`:
  `ALL` forces the new-mail scan regardless of the user's conf scan mask; `SKIP`
  suppresses it; `NOFORCE` (default, what `J` uses) defers to
  `checkMailConfScan(conf,msgBaseNum)` (572-589: tooltypes `FORCE_NEWSCAN` /
  `NO_NEWSCAN`, else the per-user `MAIL_SCAN_MASK` bit in `cb.handle[0]`).

### Control flow

```
joinConf(conf,msgBaseNum,confScan,auto,forceMailScan):
  IF checkConfAccess(conf)=FALSE THEN conf:=1                     # 4982
  IF conf out of 1..cmds.numConf THEN conf:=1                     # 4983
  WHILE conf<=numConf AND no access: conf++                       # 4984-4986  walk to first accessible
  IF conf>numConf                                                 # 4988  user can access NOTHING:
    aePuts('\b\nYou do not have access to any conferences on this BBS\b\n')   # 4989
    aePuts('Disconnecting..\b\n')                                 # 4990
    reqState:=REQ_STATE_LOGOFF; RETURN                            # 4991-4992
  IF msgBaseNum out of 1..getConfMsgBaseCount(conf): msgBaseNum:=1  # 4995  (clamps invalid prompt input)
  IF confScan=FALSE: currentConf:=conf; currentMsgBase:=msgBaseNum  # 4997-5000
  load conf name/location (5002-5004); tooltypes: NDIRS (5006), QUIET_JOIN (BBSCONFIG, 5008),
    FREEDOWNLOADS (5010), MENU_PROMPT (5012-5013), msgbase location (5015),
    REALNAME/INTERNETNAME name-type (5017-5024)
  loadMsgPointers(conf,msgBaseNum)                                # 5026
  unless conf has CUSTOM tooltype: getMailStatFile + clamp lastMsgReadConf/lastNewReadConf
    into [mailStat.lowestNotDel .. mailStat.highMsgNum], errorLog on overflow  # 5028-5050
  confScreenDir := conf dir, overridable by SCREENS tooltype       # 5052-5054

  IF confScan=FALSE                                               # 5056
    currentMenuName:=''                                           # 5057
    IF displayScreen(SCREEN_CONF_BULL) THEN doPause()              # 5058-5061  conf BULL screen + pause prompt
    relConfNum:=relConf(conf)                                     # 5063
    IF quietJoin=FALSE THEN aePuts('\b\n')                        # 5065
    IF auto      → "Auto-ReJoined" strings (5066-5075)
    ELSE (the J path):
      IF getConfMsgBaseCount(conf)>1                              # 5077
        aePuts('[32mJoining Conference[33m:[0m \s [\s]')          # 5079-5080  name [msgbase name]
        log string '\s [\s] (\d) Conference Joined'               # 5081
      ELSE
        aePuts('[32mJoining Conference[33m:[0m \s')               # 5083-5084  name only
        log string '\s (\d) Conference Joined'                    # 5085
    IF quietJoin=FALSE THEN aePuts('\b\n')                        # 5088
    callersLog('\t'+log string)                                   # 5089-5090

    IF quietJoin=FALSE                                            # 5092
      IF conf not CUSTOM:
        IF mailStat.lowestKey>1:
          '[32mMessages range from [33m( [0m\d [32m- [0m\d [33m)[0m\b\n'      # 5096-5097 (lowestKey, highMsgNum-1)
        ELSE:
          '\b\n[32mTotal messages           [33m:[0m \d\b\n'      # 5099 (highMsgNum-1)
        aePuts(that)                                              # 5101
        temp:=lastNewReadConf-1; IF temp<0 THEN temp:=1           # 5103-5104
        '\b\n[32mLast message auto scanned[33m:[0m \d\b\n'        # 5105
        '[32mLast message read        [33m:[0m \d\b\n'            # 5108 (lastMsgReadConf)
      ELSE customMsgbaseCmd(MAIL_STATS,conf,0)                    # 5111
    IF auto THEN displaySysopULStats()                            # 5115
  ENDIF

  IF auto=FALSE AND forceMailScan<>FORCE_MAILSCAN_SKIP            # 5119
    IF forceMailScan=FORCE_MAILSCAN_ALL OR checkMailConfScan(conf,msgBaseNum)  # 5120
      callMsgFuncs(MAIL_SCAN,conf,msgBaseNum)  (or customMsgbaseCmd)            # 5121-5125
      saveMsgPointers(conf,msgBaseNum)                            # 5126

  IF auto=FALSE AND confScan=FALSE                                # 5130
    bail if reqState set / carrier lost                           # 5131-5134
    loggedOnUser.confRJoin:=conf; loggedOnUser.msgBaseRJoin:=msgBaseNum        # 5135-5136  rejoin persistence
    createNodeUserFiles()                                         # 5137
ENDPROC mystat                                                    # 5139
```

### Already-in vs new conference

There is **no special-casing for rejoining the conference you are already in**. `J <current>`
re-runs the entire sequence: conf bulletin screen (+pause), "Joining Conference" line, the
message stats block, the conditional mail scan, and re-persists `confRJoin`. The only
"already in" effect is that `internalCommandJ` saved the current pointers first (25121) and
`joinConf` reloads them (5026). The genuinely different path is `auto=TRUE` (logon
auto-rejoin), which prints `Conference \d: \s Auto-ReJoined` instead and skips the scan.

---

## 4. displayScreen(SCREEN_JOINCONF) — express.e:6539, 6588-6596

Screen enum (axenums.e:19): `... SCREEN_JOIN, SCREEN_JOINCONF, SCREEN_CONF_JOINMSGBASE,
SCREEN_JOINMSGBASE, SCREEN_JOINED, ...`

```e
CASE SCREEN_JOINCONF                                       -> 6588
  StringF(screencheck,'\s\s',nodeScreenDir,'JoinConf')     -> 6589
  IF (findSecurityScreen(screencheck,screenfile)) THEN res:=displayFile(screenfile)  -> 6590
CASE SCREEN_CONF_JOINMSGBASE                               -> 6591
  StringF(screencheck,'\s\s',confScreenDir,'JoinMsgBase')  -> 6592
CASE SCREEN_JOINMSGBASE                                    -> 6594
  StringF(screencheck,'\s\s',nodeScreenDir,'JoinMsgBase')  -> 6595
```

So `J`'s conference list screen is the file `JoinConf` in the **node** screens directory,
resolved via `findSecurityScreen` (6246+): tries `<name><secLevel rounded down to 5>.<ext>`
descending in steps of 5 (per user screenType extension / `.TXT` / `.RIP` when ripMode),
then the plain non-security file — order inverted by the node `DEF_SCREENS` tooltype.
`displayScreen` returns FALSE if no file exists; `internalCommandJ` ignores the return for
SCREEN_JOINCONF (the prompt is printed regardless), but uses it for the msgbase screen
fallback (25170-25171).

---

## 5. getInverse / relative conference numbering — express.e:8568-8581

```e
PROC getInverse(cn,force=FALSE)
  IF(cn<1) THEN RETURN 0                                          -> 8571
  IF (force=FALSE) AND (sopt.toggles[TOGGLES_CONFRELATIVE]=FALSE) THEN RETURN cn   -> 8572
  -> else: count accessible conferences until the cn-th; RETURN 0 if exhausted     -> 8573-8581
```

- `TOGGLES_CONFRELATIVE` (axcommon.e:367) is enabled by the `RELATIVE_CONFERENCES`
  BBSCONFIG tooltype (ACP.e:2698-2702).
- Toggle OFF (default): the J argument is the absolute conference number.
- Toggle ON: the argument counts only conferences the user can access; numbers past the
  accessible count map to 0 (→ clamped to 1 by 25153).

`checkConfAccess` (8499-8512): per-user access string — `user.conferenceAccess[confNum-1]="X"`
— or, for named access areas, the `Conf.<n>` tooltype in the area definition.

---

## 6. lineInput — express.e:2170-2380 (as used at 25145/25174)

`lineInput(promptText,defaultOutput,maxLen,timeout,outputString,allowHistory=TRUE)`

- Call: `lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)` — prompt printed first (2178-2180),
  empty default, **maxLen 5** characters (chars beyond that are ignored, 2335),
  `INPUT_TIMEOUT=300` seconds (axconsts.e:89).
- Timeout shaping (2185-2198): `TOGGLES_NOTIMEOUT` tooltype or `ACS_NO_TIMEOUT` → no
  timeout; otherwise for timeouts ≥120s the wait is `timeout-60`, and the **first** expiry
  sends a BELL and grants a 60-second grace (2224-2230) before returning `RESULT_TIMEOUT`.
- Line editing: backspace/delete, CTRL-X clears, left/right arrows, up/down history
  (CTRL-B clears history) — 2235-2351.
- Terminates on CR (`ch=13`, 2355) → `RESULT_SUCCESS` (2359). Abort/carrier-loss pass
  through as `RESULT_ABORT`/`RESULT_NO_CARRIER` (2211-2215).
- Always emits `'\b\n'` after input completes (2378) — this is the newline seen after the
  user presses Enter on a blank prompt.
- Non-empty successful input is appended to the recall history and (if input logging)
  callers log (2360-2366).

`Val` is the Amiga E built-in: parses an optional leading sign and decimal (or `$`hex /
`%`binary) digits at the start of the string and stops at the first non-numeric character
(hence `"2.3"` → 2); a string with no leading number yields 0.

---

## Verbatim strings

| # | express.e line | String (verbatim, E escapes preserved) | Emitted via |
|---|---|---|---|
| 1 | 25144 | `'Conference Number (1-\d): '` (`\d` = `cmds.numConf`) | lineInput prompt |
| 2 | 25157 | `'\b\nYou do not have access to the requested conference\b\n\b\n'` | aePuts |
| 3 | 25173 | `'Message Base Number (1-\d): '` (`\d` = msgbase count of target conf) | lineInput prompt |
| 4 | 4989 | `'\b\nYou do not have access to any conferences on this BBS\b\n'` | aePuts (joinConf) |
| 5 | 4990 | `'Disconnecting..\b\n'` | aePuts (joinConf) |
| 6 | 5079 | `'[32mJoining Conference[33m:[0m \s [\s]'` (conf name, msgbase name; multi-msgbase confs) | aePuts unless QUIET_JOIN |
| 7 | 5083 | `'[32mJoining Conference[33m:[0m \s'` (conf name; single-msgbase confs) | aePuts unless QUIET_JOIN |
| 8 | 5081 | `'\s [\s] (\d) Conference Joined'` | callersLog only (prefixed `\t`, 5089) |
| 9 | 5085 | `'\s (\d) Conference Joined'` | callersLog only |
| 10 | 5071 | `'Conference \d: \s [\s] Auto-ReJoined'` (auto=TRUE path) | aePuts unless QUIET_JOIN |
| 11 | 5073 | `'Conference \d: \s Auto-ReJoined'` (auto=TRUE path) | aePuts unless QUIET_JOIN |
| 12 | 5096-5097 | `'[32mMessages range from [33m( [0m\d [32m- [0m\d [33m)[0m\b\n'` (lowestKey, highMsgNum-1) | aePuts |
| 13 | 5099 | `'\b\n[32mTotal messages           [33m:[0m \d\b\n'` (highMsgNum-1) | aePuts |
| 14 | 5105 | `'\b\n[32mLast message auto scanned[33m:[0m \d\b\n'` (lastNewReadConf-1, floored to 1) | aePuts |
| 15 | 5108 | `'[32mLast message read        [33m:[0m \d\b\n'` (lastMsgReadConf) | aePuts |
| 16 | 5065 / 5088 | `'\b\n'` before and after the joining announcement | aePuts unless QUIET_JOIN |
| 17 | 25162 | `'****Conference Location unknown in MENU routines****'` | callersLog only |
| 18 | 25163 | `'**** For Conference \d'` | callersLog only |
| 19 | 3038 | `'\b\nCommand requires higher access.\b\n'` (on RESULT_NOT_ALLOWED) | aePuts (higherAccess) |
| 20 | 5144 | `'\b\n[32m([33mPause[32m)[34m...[32mSpace To Resume[33m: [0m'` (doPause, after conf BULL screen) | aePuts |

Bracket sequences `[32m`, `[33m`, `[0m`, `[34m` are preceded by a literal ESC (0x1B) byte
in the source file.

---

## Open questions (need live observation / runtime config)

1. **`Val` sign handling**: `Val` is an E built-in (no source in this repo). Whether
   `"+2"` parses as 2 or as 0 (prompt branch) needs a live check; `"-1"` definitely
   parses negative (→ prompt branch).
2. **`cmds.numConf`** value, and whether `RELATIVE_CONFERENCES` is set, are runtime
   BBSCONFIG state — they change both the prompt's `N` and the clamping behaviour of
   out-of-range prompt input (clamp-to-numConf vs clamp-to-1). Verify both modes live.
3. **`QUIET_JOIN`** BBSCONFIG tooltype suppresses all join announcement/stat output
   (5008, 5065, 5075/5080/5084, 5088, 5092) — default install state unknown.
4. Whether a `JoinConf` screen file ships in the reference BBS data (if absent,
   `displayScreen(SCREEN_JOINCONF)` silently shows nothing and the user only sees the
   prompt). Same for `JoinMsgBase`.
5. Exact wire bytes of the lineInput timeout warning (BELL then grace) and the prompt
   echo behaviour with the 5-char limit — best confirmed against the FS-UAE reference.
6. `mailStat` numbers in the stats block (`highMsgNum-1` can be `-1` on a fresh empty
   base if `getMailStatFile` fails → values zeroed at 5031-5035; the displayed
   "Total messages" would then be `-1`). Confirm live what an empty msgbase shows.
