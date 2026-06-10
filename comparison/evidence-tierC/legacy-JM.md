# Legacy evidence: `JM` (join message base) — slices C4a / C4b

Source: `/Users/paul/Documents/GitHub/nextexpress/amiexpress/express.e` (line numbers from that file
unless another file is named). Supporting files: `axenums.e`, `axconsts.e`, `axcommon.e`,
`tooltypes.e` in the same directory.

Notation: `\b\n` is the Amiga E end-of-line escape (CR+LF on the telnet wire). `\e` below denotes a
raw ESC byte (0x1B) that is embedded literally in the source string before `[32m` etc. — verified
with `od -c` (e.g. line 5079 contains `' 033 [ 3 2 m J o i n i n g ...'`). `\d`, `\s`, `\l\s[n]` are
E `StringF` format codes (decimal, string, left-justified padded string).

---

## 1. Dispatch table entry (express.e:28320–28321)

`processCommand` (`express.e:28229`) splits the typed line at the **first space** (28236–28244),
uppercases only the command token (28245: `UpperStr(cmdcode)`), and everything after the first
space is passed through as one raw `cmdparams` string. After sys/bbs command lookups,
`processInternalCommand` (28285) dispatches:

```
28320  ELSEIF (StrCmp(cmdcode,'JM'))
28321    res:=internalCommandJM(cmdparams)
```

For comparison, `J` is at 28318–28319 → `internalCommandJ(cmdparams)`.

If the returned result is `RESULT_NOT_ALLOWED` and the command was user-typed (`privcmd=FALSE`),
line 28400 calls `higherAccess()`:

```
3038  aePuts('\b\nCommand requires higher access.\b\n')
```

Result codes (`axenums.e:23`): `RESULT_FAILURE=-1, RESULT_SUCCESS=0, RESULT_NOT_ALLOWED=1, ...,
RESULT_TIMEOUT=-3, RESULT_NO_CARRIER=-4`.

---

## 2. internalCommandJM — full control flow (express.e:25185–25237)

```
PROC internalCommandJM(params)                                   -> 25185
  DEF newStr[5]:STRING                                           -> 25186 (input buffer: max 5 chars)
  ...
  IF checkSecurity(ACS_JOIN_CONFERENCE)=FALSE
      THEN RETURN RESULT_NOT_ALLOWED                             -> 25191 (caller then prints
                                                                    "Command requires higher access.")
  saveMsgPointers(currentConf,currentMsgBase)                    -> 25193 (persist read pointers of
                                                                    the base being left)
  setEnvStat(ENV_JOIN)                                           -> 25195 (node status -> "JOINING CONF",
                                                                    see 24324-24326)
  parseParams(params)                                            -> 25197 (space-tokenise into parsedParams)

  newMsgBase:=-1                                                 -> 25199
  IF parsedParams.count()>0                                      -> 25200
    param:=parsedParams.item(0)                                  -> 25201
    IF (InStr(param,'.')>=0)                                     -> 25203  ** dotted arg **
      internalCommandJ(params)                                   -> 25204  delegate, passing the RAW
                                                                    params string (not just item 0)
      RETURN                                                     -> 25205  bare RETURN: returns 0
                                                                    (= RESULT_SUCCESS), J's own status
                                                                    is DISCARDED
    ENDIF
    IF StrLen(param)>0 THEN newMsgBase:=Val(param)               -> 25208 (E Val: non-numeric -> 0)
  ENDIF

  cnt:=readToolTypeInt(TOOLTYPE_MSGBASE,currentConf,'NMSGBASES') -> 25211
  IF cnt=-1                                                      -> 25212 (NMSGBASES tooltype absent)
    aePuts('\b\nThis conference does not contain multiple message bases\b\n\b\n')   -> 25213
    RETURN RESULT_FAILURE                                        -> 25214
  ENDIF

  cnt:=getConfMsgBaseCount(currentConf)                          -> 25218 (re-read; -1 mapped to 1,
                                                                    see 2048-2052)

  IF (newMsgBase<1) OR (newMsgBase>cnt)                          -> 25220  ** no-arg OR out-of-range:
                                                                    interactive prompt **
    IF displayScreen(SCREEN_CONF_JOINMSGBASE)=FALSE              -> 25221 (conf-local screen first)
      displayScreen(SCREEN_JOINMSGBASE)                          -> 25222 (node-level fallback)
    ENDIF
    StringF(tempStr,'Message Base Number (1-\d): ',cnt)          -> 25224 (upper bound = msg-base
                                                                    count of the CURRENT conference)
    stat:=lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)           -> 25225 (max 5 chars, 300 s timeout)
    IF stat<>RESULT_SUCCESS THEN RETURN stat                     -> 25226 (timeout / carrier loss
                                                                    propagates)
    IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS               -> 25228 (blank Enter = silent abort,
                                                                    stay where you are)
    newMsgBase:=Val(newStr)                                      -> 25230 (single shot — NO re-prompt
                                                                    loop)
  ENDIF

  IF newMsgBase<1 THEN newMsgBase:=1                             -> 25233  clamp low  (also catches
                                                                    non-numeric prompt input, Val->0)
  IF newMsgBase>cnt THEN newMsgBase:=cnt                         -> 25234  clamp high

  joinConf(currentConf,newMsgBase,FALSE,FALSE)                   -> 25236 (confScan=FALSE, auto=FALSE,
                                                                    forceMailScan defaults NOFORCE)
ENDPROC RESULT_SUCCESS                                           -> 25237
```

### Argument parsing summary (C4a)

- `JM 2` → `newMsgBase=2`. Valid range is `1..cnt` where
  `cnt = getConfMsgBaseCount(currentConf)`.
- Out-of-range (`JM 0`, `JM 99`) does **not** clamp directly — it falls into the interactive prompt
  (25220). Clamping (25233–25234) only applies to what comes **out of the prompt**.
- Non-numeric arg (`JM x`): `Val('x')=0` → `<1` → interactive prompt.
- `JM` with no arg: `newMsgBase=-1` → interactive prompt.
- Prompt input handling: blank → silent return (no message, no join); non-numeric → `Val`=0 →
  clamped to 1 → joins base 1; numeric above `cnt` → clamped to `cnt`. **Single-shot — the legacy
  never loops back to re-prompt.**
- After the prompt, `lineInput` itself echoes one `\b\n` (2378) before control returns.

### Screen lookup (express.e:6591–6596)

- `SCREEN_CONF_JOINMSGBASE` → file `<confScreenDir>JoinMsgBase` (6592)
- `SCREEN_JOINMSGBASE` → file `<nodeScreenDir>JoinMsgBase` (6595)
- Both go through `findSecurityScreen` (security-suffixed screen variants) and `displayFile`. If
  neither file exists, nothing is shown and the prompt appears alone.

---

## 3. Dotted-argument handling — verified

**The SLICES.md / `slices/cmds-conf-nav.md:58-59` claim is CORRECT.** `JM` itself does *not*
interpret the dot. If the first token contains a `.` anywhere (`InStr(param,'.')>=0`, 25203),
`internalCommandJM` hands the *entire original* `params` string to `internalCommandJ` (25204) and
returns. The dot is parsed in **`internalCommandJ`** (25113):

```
25130  IF StrLen(param)>0
25131    newConf:=Val(param)                  -> Val stops at the '.' : "2.3" -> conf 2
25132    IF (pos:=InStr(param,'.'))>=0
25133      newMsgBase:=Val(param+pos+1)       -> text after the dot: "2.3" -> msgbase 3
25134    ELSEIF parsedParams.count()>1
25135      newMsgBase:=Val(parsedParams.item(1))   -> "J 2 3" two-arg form, dot wins if both present
25136    ENDIF
25137  ENDIF
```

So in the legacy, dotted args are a **`J` feature**; `JM x.y` works only by delegation. Meanings:
part before the dot = conference number (subject to `getInverse` relative-numbering mapping,
25140), part after the dot = message base number within that conference. Notes:

- `J 2.` → `Val('')=0` → msgbase 0 → out of range → J's own message-base prompt (25169–25180).
- Because the dotted check (25203) precedes the NMSGBASES check (25212), `JM 2.3` works even when
  the *current* conference has no NMSGBASES tooltype.
- The bare `RETURN` at 25205 returns 0 (`RESULT_SUCCESS`), discarding whatever
  `internalCommandJ` returned (including `RESULT_TIMEOUT`/`RESULT_NO_CARRIER` from its prompts).
- In `internalCommandJ`, the message-base value from a dotted/second arg that is out of range
  triggers J's message-base prompt whose result is **not** clamped (25179) — `joinConf` then
  resets any out-of-range base to 1 (4995). Contrast with JM, which clamps to `cnt` (25234).

For reference, `internalCommandJ`'s own conference prompt (used when its arg is missing/out of
range, 25142–25151):

```
25143  displayScreen(SCREEN_JOINCONF)                      -> file <nodeScreenDir>JoinConf (6589)
25144  StringF(tempStr,'Conference Number (1-\d): ',cmds.numConf)
25145  stat:=lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)
25146  IF stat<>RESULT_SUCCESS THEN RETURN stat
25148  IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS
25150  newConf:=getInverse(Val(newStr))
25153  IF newConf<1 THEN newConf:=1
25154  IF newConf>cmds.numConf THEN newConf:=cmds.numConf
25156  IF(checkConfAccess(newConf)=FALSE)
25157    aePuts('\b\nYou do not have access to the requested conference\b\n\b\n')
25158    RETURN RESULT_FAILURE
```

---

## 4. joinConf — what actually happens on join (express.e:4975–5139)

`joinConf(conf, msgBaseNum, confScan, auto, forceMailScan=FORCE_MAILSCAN_NOFORCE)`. JM calls it as
`joinConf(currentConf, newMsgBase, FALSE, FALSE)`.

Pseudocode with line refs (paths relevant to JM: `confScan=FALSE`, `auto=FALSE`):

```
4982  IF checkConfAccess(conf)=FALSE THEN conf:=1
4983  IF conf<1 OR conf>cmds.numConf THEN conf:=1
4984  WHILE conf<=numConf AND no access: conf++          -> walk forward to first accessible conf
4988  IF conf>numConf
4989    aePuts('\b\nYou do not have access to any conferences on this BBS\b\n')
4990    aePuts('Disconnecting..\b\n')
4991    reqState:=REQ_STATE_LOGOFF ; RETURN
4995  IF msgBaseNum<1 OR >getConfMsgBaseCount(conf) THEN msgBaseNum:=1   -> final safety clamp to 1
4998  currentConf:=conf ; currentMsgBase:=msgBaseNum     -> state change (confScan=FALSE)
5002  refresh currentConfName / currentConfDir
5006  maxDirs := TOOLTYPE_CONF NDIRS
5008  quietJoin := TOOLTYPE_BBSCONFIG QUIET_JOIN exists
5010  freeDownloads := TOOLTYPE_CONF FREEDOWNLOADS exists
5012  menuPrompt := TOOLTYPE_CONF MENU_PROMPT (cleared first)
5015  msgBaseLocation := getMsgBaseLocation(conf,msgBaseNum)
5017  confNameType := username / REALNAME(.n) / INTERNETNAME(.n) tooltypes
5026  loadMsgPointers(conf,msgBaseNum)                   -> load read pointers for the new base
5028  IF conf not CUSTOM:
5029    getMailStatFile(conf,msgBaseNum); on failure zero mailStat + last-read pointers (5030-5036)
5037    floor lastMsgReadConf/lastNewReadConf at mailStat.lowestNotDel
5040    IF lastMsgReadConf>highMsgNum -> errorLog 'error setting last message read: value \d, high msg num \d' (5041), reset 0
5045    IF lastNewReadConf>highMsgNum -> errorLog 'error setting last new read read: value \d, high msg num \d' (5046), reset 0
5052  confScreenDir := conf dir, overridden by TOOLTYPE_CONF SCREENS

      -- visible output starts here (confScan=FALSE) --
5057  StrCopy(currentMenuName,'')
5058  IF displayScreen(SCREEN_CONF_BULL)                 -> conf bulletin screen, if present
5059    doPause()  -> '\b\n\e[32m(\e[33mPause\e[32m)\e[34m...\e[32mSpace To Resume\e[33m: \e[0m' (5144)
5063  relConfNum:=relConf(conf)
5065  IF quietJoin=FALSE THEN aePuts('\b\n')
      (auto=FALSE branch, 5076-5087:)
5077  IF getConfMsgBaseCount(conf)>1
5078    getMsgBaseName(conf,msgBaseNum,tempstr)          -> TOOLTYPE_MSGBASE 'NAME.<n>' (2054-2059)
5079    aePuts('\e[32mJoining Conference\e[33m:\e[0m \s [\s]')   -> conf name, msg base name (if not quiet)
5081    log string := '\s [\s] (\d) Conference Joined'
      ELSE
5083    aePuts('\e[32mJoining Conference\e[33m:\e[0m \s')        -> conf name only (if not quiet)
5085    log string := '\s (\d) Conference Joined'
5088  IF quietJoin=FALSE THEN aePuts('\b\n')
5089  callersLog('\t'+log string)
5092  IF quietJoin=FALSE AND conf not CUSTOM:
5094    IF mailStat.lowestKey>1
5096      aePuts('\e[32mMessages range from \e[33m( \e[0m\d \e[32m- \e[0m\d \e[33m)\e[0m\b\n')
              -> lowestKey, highMsgNum-1
      ELSE
5099      aePuts('\b\n\e[32mTotal messages           \e[33m:\e[0m \d\b\n')  -> highMsgNum-1
5103    temp:=lastNewReadConf-1 ; IF temp<0 THEN temp:=1
5105    aePuts('\b\n\e[32mLast message auto scanned\e[33m:\e[0m \d\b\n')    -> temp
5108    aePuts('\e[32mLast message read        \e[33m:\e[0m \d\b\n')        -> lastMsgReadConf
5110  ELSE (CUSTOM conf): customMsgbaseCmd(MAIL_STATS,conf,0)               -> 5111

      -- mail scan (auto=FALSE, forceMailScan=NOFORCE) --
5119  IF forceMailScan<>SKIP
5120    IF forceMailScan=ALL OR checkMailConfScan(conf,msgBaseNum)
5122      callMsgFuncs(MAIL_SCAN,conf,msgBaseNum)        -> the actual new-mail scan
5126      saveMsgPointers(conf,msgBaseNum)

      -- rejoin bookkeeping (auto=FALSE, confScan=FALSE) --
5131  IF reqState<>REQ_STATE_NONE THEN RETURN mystat
5132  IF remote logon AND carrier lost THEN RETURN mystat
5135  loggedOnUser.confRJoin:=conf                       -> persisted auto-rejoin conference
5136  loggedOnUser.msgBaseRJoin:=msgBaseNum              -> persisted auto-rejoin message base
5137  createNodeUserFiles()
5139  ENDPROC mystat
```

**Mail scan trigger:** yes — joining via JM runs a mail scan when `checkMailConfScan(conf,msgBase)`
(express.e:572–589) says so: `TOOLTYPE_CONF FORCE_NEWSCAN` forces TRUE, `NO_NEWSCAN` forces FALSE,
otherwise the per-user conf-base mail-scan mask bit (`cb.handle[0] AND MAIL_SCAN_MASK`, default
TRUE when no record).

**State changed by a JM join:** `currentMsgBase` (and `currentConf` re-set to the same value),
`currentConfName/currentConfDir/confScreenDir/msgBaseLocation/menuPrompt/maxDirs/freeDownloads/
confNameType`, message read pointers (saved for old base at 25193, loaded for new at 5026),
`mailStat`, `relConfNum`, `loggedOnUser.confRJoin/msgBaseRJoin`, node user files, callers log entry.

**Joining the message base you are already in:** there is **no** "already there" check anywhere in
`internalCommandJM` or `joinConf`. `JM <current>` performs the complete re-join: pointers saved and
reloaded, CONF_BULL screen + pause, the full "Joining Conference" banner and message stats, mail
scan check, rejoin bookkeeping. Identical output to joining any other base.

---

## 5. JM on a conference with exactly one message base

The deciding factor is the **`NMSGBASES` tooltype on the conference icon**, not the count itself:

- **NMSGBASES absent** (the normal single-base setup): `readToolTypeInt` (tooltypes.e:176–181)
  returns `-1` → JM prints
  `'\b\nThis conference does not contain multiple message bases\b\n\b\n'` (25213) and returns
  `RESULT_FAILURE`. This applies to **both** the arg form (`JM 1`) and the no-arg form, because the
  check (25212) sits before the prompt (25220). Only the dotted form escapes it (delegated at
  25204 before the check).
- **NMSGBASES=1 explicitly configured**: `cnt=1`, no error. `JM 1` re-joins base 1 (full join
  sequence). `JM`/`JM 2` prompts with `'Message Base Number (1-1): '`; prompt output then clamps
  to 1.
- Edge case: NMSGBASES set to `0` or a non-numeric value → `Val`→0 ≠ -1, so no error message;
  `getConfMsgBaseCount` keeps `0` (2051 only remaps `-1`) → prompt reads
  `'Message Base Number (1-0): '` and any join clamps oddly. Misconfiguration, noting for
  completeness only.
- Note the asymmetry with `joinConf`'s banner: the bracketed `[msg base name]` variant is chosen by
  `getConfMsgBaseCount(conf)>1` (5077), so with NMSGBASES=1 the banner is the plain
  `Joining Conference: <name>` form even though JM accepted the command.

`<<` / `>>` (C4b context, 24566–24592): both call `saveMsgPointers` + `setEnvStat(ENV_JOIN)` but
**no security check**, compute `currentMsgBase∓1`, and call `joinConf(currentConf,newMsgBase,
FALSE,FALSE)` directly when in range; when stepping past either end they fall back to
`internalCommandJM('')` (24574, 24588) — i.e. the NMSGBASES error or the interactive prompt.

---

## 6. Verbatim strings (wire output unless noted)

| Line | String (verbatim; `\e` = raw ESC 0x1B) | Emitted by / when |
|---|---|---|
| 25213 | `\b\nThis conference does not contain multiple message bases\b\n\b\n` | JM, conf lacks NMSGBASES tooltype |
| 25224 | `Message Base Number (1-\d): ` | JM interactive prompt; `\d`=getConfMsgBaseCount(currentConf) |
| 25144 | `Conference Number (1-\d): ` | J interactive prompt; `\d`=cmds.numConf (raw count, not access-filtered) |
| 25173 | `Message Base Number (1-\d): ` | J msg-base prompt (only when explicit msgbase out of range); `\d`=getConfMsgBaseCount(newConf) |
| 25157 | `\b\nYou do not have access to the requested conference\b\n\b\n` | J, checkConfAccess fail |
| 3038 | `\b\nCommand requires higher access.\b\n` | dispatcher, when JM/J return RESULT_NOT_ALLOWED (ACS_JOIN_CONFERENCE denied) |
| 4989 | `\b\nYou do not have access to any conferences on this BBS\b\n` | joinConf, no accessible conf at all |
| 4990 | `Disconnecting..\b\n` | joinConf, follows 4989; sets REQ_STATE_LOGOFF |
| 5065 | `\b\n` | joinConf, before banner (quietJoin=FALSE) |
| 5079 | `\e[32mJoining Conference\e[33m:\e[0m \s [\s]` | joinConf banner, multi-base conf; `\s`=conf name, `\s`=msg base name |
| 5083 | `\e[32mJoining Conference\e[33m:\e[0m \s` | joinConf banner, single-base conf |
| 5088 | `\b\n` | joinConf, after banner (quietJoin=FALSE) |
| 5096–5097 | `\e[32mMessages range from \e[33m( \e[0m\d \e[32m- \e[0m\d \e[33m)\e[0m\b\n` | joinConf, mailStat.lowestKey>1; args lowestKey, highMsgNum-1 |
| 5099 | `\b\n\e[32mTotal messages           \e[33m:\e[0m \d\b\n` | joinConf, lowestKey<=1; arg highMsgNum-1 |
| 5105 | `\b\n\e[32mLast message auto scanned\e[33m:\e[0m \d\b\n` | joinConf; arg max(lastNewReadConf-1,1) |
| 5108 | `\e[32mLast message read        \e[33m:\e[0m \d\b\n` | joinConf; arg lastMsgReadConf |
| 5144 | `\b\n\e[32m(\e[33mPause\e[32m)\e[34m...\e[32mSpace To Resume\e[33m: \e[0m` | doPause after CONF_BULL screen, if the screen file exists |
| 5071 | `Conference \d: \s [\s] Auto-ReJoined` | joinConf auto=TRUE only (login rejoin, not JM) |
| 5073 | `Conference \d: \s Auto-ReJoined` | joinConf auto=TRUE only |
| 5081 | `\s [\s] (\d) Conference Joined` | callers log only (prefixed `\t`, 5089) |
| 5085 | `\s (\d) Conference Joined` | callers log only |
| 25162 | `****Conference Location unknown in MENU routines****` | J only, callers log, conf location empty |
| 25163 | `**** For Conference \d` | J only, callers log |
| 2378 | `\b\n` | lineInput epilogue after Enter/timeout (every prompt above) |

Screen files displayed (content site-configurable, not in source):
`<confScreenDir>JoinMsgBase` else `<nodeScreenDir>JoinMsgBase` before the JM prompt (6592/6595);
`<nodeScreenDir>JoinConf` before the J prompt (6589); `<confScreenDir>` conf bulletin
(SCREEN_CONF_BULL) inside joinConf (5058).

---

## 7. Open questions (need live observation / runtime config)

1. **Screen file presence on the reference board** — whether `JoinMsgBase` / `JoinConf` /
   conf-bulletin screen files exist in the test BBS data, and their exact contents (these are data
   files, not source strings). Determines what precedes the prompts and whether the Pause prompt
   appears.
2. **QUIET_JOIN tooltype** — if set on the reference board, the entire banner + stats block is
   suppressed (5065/5075/5080/5084/5088/5092 all gated on `quietJoin=FALSE`). Needs checking in the
   live config.
3. **Relative conference numbering** (`TOGGLES_CONFRELATIVE`, getInverse/relConf 8558–8581) — does
   the reference board run with it on? It changes how the dotted/numeric conference part of `J x.y`
   is interpreted and makes the `Conference Number (1-\d)` upper bound (raw `cmds.numConf`)
   inconsistent with accepted relative input. Affects J more than JM.
4. **Mail-scan output during JM join** — `callMsgFuncs(MAIL_SCAN,...)` (5122) output depends on
   new-mail state and per-user scan mask; the exact scan transcript needs a live capture.
5. **`joinConf` return value when no mail-stat file is read** — `mystat` (4980) is only assigned by
   `getMailStatFile`/`callMsgFuncs`; for a CUSTOM conf with no scan it is whatever an
   uninitialised E local holds. Irrelevant to JM (return discarded at 25236) but flagged in case
   another caller inspects it.
6. **Amiga E `Val` corner cases** — assumed standard E semantics (skips leading whitespace, parses
   optional sign / `$`hex / `%`binary, returns 0 on no digits, stops at first non-numeric char such
   as `.`). Worth one live probe (`JM $2`, `JM -1`) if exactness matters for the port.
7. **NMSGBASES=0 / non-numeric** edge case (prompt reading `(1-0)`) — behaviour derived from code
   only; confirm live before encoding it in tests, or treat as out-of-scope misconfiguration.
