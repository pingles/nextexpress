# Legacy evidence: `<` / `>` (prev/next conference) and `<<` / `>>` (prev/next message base)

Source: `/Users/paul/Documents/GitHub/nextexpress/amiexpress/express.e` (plus
`axcommon.e`, `axenums.e`, `tooltypes.e` in the same directory). All line numbers
refer to those files. Parity slices: C3 (`<`/`>`), C4b (`<<`/`>>`).

Notation: `\b\n` is the Amiga E end-of-line escape in string literals (CR+LF on the
telnet wire). `ESC` denotes a literal 0x1B byte present in the source file before
`[32m`-style ANSI sequences (verified with `cat -v`: source shows `^[[32m`).

---

## 1. Tokenizer and dispatch

### Tokenization — `processCommand`, express.e:28229-28256

```e
28229  PROC processCommand(cmdtext,allowsyscmd=FALSE, subtype=-1)
28234    IF StrLen(cmdtext)=0 THEN RETURN RESULT_SUCCESS
28236    spacepos:=InStr(cmdtext,' ')
28238    IF spacepos>=0
28239      midStr2(cmdcode,cmdtext,0,spacepos)      -> cmdcode = text before first space
28240      MidStr(cmdparams,cmdtext,spacepos+1)     -> cmdparams = text after first space
28241    ELSE
28242      StrCopy(cmdcode,cmdtext)
28243      StrCopy(cmdparams,'')
28244    ENDIF
28245    UpperStr(cmdcode)
       ...
28248    IF (subtype<SUBTYPE_INTCMD)               -> sys/bbs (door) commands tried FIRST
28249      IF allowsyscmd ... runSysCommand(...) ...
28253      IF (res:=runBbsCommand(cmdcode,cmdparams,TRUE,subtype))=RESULT_SUCCESS THEN RETURN RESULT_SUCCESS
       ...
28256  ENDPROC processInternalCommand(cmdcode,cmdparams)
```

Key facts:

- The command token is **everything up to the first space**. `<<` typed at the
  menu prompt is a single two-character token; the second `<` is **part of the
  command token, not an argument**. `< <` would instead be token `<` with param `<`.
- Dispatch is an exact full-token `StrCmp` — there is no prefix matching, so `<`
  and `<<` are entirely distinct entries (see below).
- The menu loop reads a whole line: `lineInput('','',255,INPUT_TIMEOUT,commandText)`
  at express.e:28624, upper-cases it (`UpperStr(commandText)`, :28645) and calls
  `processCommand(commandText)` (:28646). (Upper-casing is a no-op for `<`/`>`.)
- Custom sys/BBS door commands are tried before internal dispatch (:28248-28254),
  so a sysop-installed command named `<` could shadow the internal one (runtime
  config dependent).

### Dispatch table — `processInternalCommand`, express.e:28285+

```e
28322    ELSEIF (StrCmp(cmdcode,'<'))
28323      res:=internalCommandLT()
28324    ELSEIF (StrCmp(cmdcode,'<<'))
28325      res:=internalCommandLT2()
28326    ELSEIF (StrCmp(cmdcode,'>'))
28327      res:=internalCommandGT()
28328    ELSEIF (StrCmp(cmdcode,'>>'))
28329      res:=internalCommandGT2()
```

Note none of the four handlers take `cmdparams` — any arguments typed after a
space are silently discarded.

Unknown-command and not-allowed handling at the end of `processInternalCommand`:

```e
28386    ELSEIF privcmd=FALSE
28387      aePuts('\b\nNo such command!!  Use ''?'' for command list.\b\n\b\n')
28388    ENDIF
28390    IF ((res=RESULT_NOT_ALLOWED) AND (privcmd=FALSE)) THEN higherAccess()
```

`higherAccess()` (express.e:3037-3039) prints:

```
'\b\nCommand requires higher access.\b\n'        (express.e:3038)
```

---

## 2. The four handlers (express.e:24529-24592)

### `<` — `internalCommandLT()` express.e:24529-24546

```e
24529  PROC internalCommandLT()
24530    DEF newConf
24531    IF checkSecurity(ACS_JOIN_CONFERENCE)=FALSE THEN RETURN RESULT_NOT_ALLOWED
24532    saveMsgPointers(currentConf,currentMsgBase)
24534    setEnvStat(ENV_JOIN)
24535    newConf:=currentConf-1
24536    WHILE (newConf>0) AND (checkConfAccess(newConf)=FALSE)
24537      newConf--
24538    ENDWHILE
24540    IF newConf<1
24541      internalCommandJ('')
24542    ELSE
24543      joinConf(newConf,1,FALSE,FALSE)
24544    ENDIF
24546  ENDPROC RESULT_SUCCESS
```

- Security: `ACS_JOIN_CONFERENCE` (enum index 7, axcommon.e:12). Failure returns
  `RESULT_NOT_ALLOWED` → caller prints `higherAccess()` message (above).
- Start: `currentConf - 1`, walking **downward** in **absolute** conference
  numbers, skipping any conference where `checkConfAccess(newConf)=FALSE`.
- Bounds: **no wraparound**. If the walk falls off the bottom (`newConf<1`,
  i.e. already at the lowest accessible conference), it falls back to
  `internalCommandJ('')` — the **interactive join-conference prompt**
  (confirms the SLICES.md claim at :24536-24544).
- Success: `joinConf(newConf, 1, FALSE, FALSE)` — joins **message base 1** of the
  new conference, `confScan=FALSE`, `auto=FALSE`, default
  `forceMailScan=FORCE_MAILSCAN_NOFORCE`.
- Emits no strings of its own; all output comes from `joinConf` (or from the J
  prompt fallback).

### `>` — `internalCommandGT()` express.e:24548-24564

```e
24548  PROC internalCommandGT()
24550    IF checkSecurity(ACS_JOIN_CONFERENCE)=FALSE THEN RETURN RESULT_NOT_ALLOWED
24551    saveMsgPointers(currentConf,currentMsgBase)
24553    setEnvStat(ENV_JOIN)
24554    newConf:=currentConf+1
24555    WHILE (newConf<=cmds.numConf) AND (checkConfAccess(newConf)=FALSE)
24556      newConf++
24557    ENDWHILE
24559    IF newConf>cmds.numConf
24560      internalCommandJ('')
24561    ELSE
24562      joinConf(newConf,1,FALSE,FALSE)
24563    ENDIF
24564  ENDPROC RESULT_SUCCESS
```

Mirror image of `<`: walks **upward**, bounded by `cmds.numConf` (total
configured conferences). Past the top → `internalCommandJ('')` interactive
prompt (SLICES.md claim at :24555-24563 confirmed). Success → message base 1.

### `<<` — `internalCommandLT2()` express.e:24566-24578

```e
24566  PROC internalCommandLT2()
24568    saveMsgPointers(currentConf,currentMsgBase)
24570    setEnvStat(ENV_JOIN)
24571    newMsgBase:=currentMsgBase-1
24573    IF newMsgBase<1
24574      internalCommandJM('')
24575    ELSE
24576      joinConf(currentConf,newMsgBase,FALSE,FALSE)
24577    ENDIF
24578  ENDPROC RESULT_SUCCESS
```

- **No `checkSecurity(ACS_JOIN_CONFERENCE)` check in this handler** (unlike
  `<`/`>`). The check only happens if it falls into the `internalCommandJM('')`
  branch (JM checks it at :25191). So a user without ACS_JOIN_CONFERENCE can
  still move `currentMsgBase-1 → joinConf` directly.
- No access walk at all: there is **no per-message-base access check** anywhere;
  it simply decrements.
- `currentMsgBase=1` (or 0) → `internalCommandJM('')` — the **interactive
  join-message-base prompt** (see §4). No wraparound.
- Success: `joinConf(currentConf, newMsgBase, FALSE, FALSE)` — re-joins the
  *same* conference with the new message base number.

### `>>` — `internalCommandGT2()` express.e:24580-24592

```e
24580  PROC internalCommandGT2()
24582    saveMsgPointers(currentConf,currentMsgBase)
24584    setEnvStat(ENV_JOIN)
24585    newMsgBase:=currentMsgBase+1
24587    IF newMsgBase>getConfMsgBaseCount(currentConf)
24588      internalCommandJM('')
24589    ELSE
24590      joinConf(currentConf,newMsgBase,FALSE,FALSE)
24591    ENDIF
24592  ENDPROC RESULT_SUCCESS
```

Mirror of `<<`: increments, bounded by `getConfMsgBaseCount(currentConf)`.
Past the last base → `internalCommandJM('')`. Same security quirk: no
`ACS_JOIN_CONFERENCE` check on the direct-join path.

All four return `RESULT_SUCCESS` from the handler itself regardless of what the
fallback prompt did.

### Shared side effects

- `saveMsgPointers(currentConf,currentMsgBase)` (express.e:4916): persists the
  current conference/msgbase read pointers and (with `ACS_CONFERENCE_ACCOUNTING`)
  byte counters into the user's conf DB before moving. Early-outs if slot<=0 or
  conf/msgbase <1 (:24921-24925). No wire output.
- `setEnvStat(ENV_JOIN)` (express.e:13184): updates the node status shown on the
  ACP/WHO display to `'JOINING CONF'` (express.e:24326). `ENV_JOIN=16`
  (axcommon.e:24). No wire output to the user.

---

## 3. `checkConfAccess` — express.e:8499-8512

```e
8499  PROC checkConfAccess(confNum,user=NIL:PTR TO user)
8501    IF user=NIL THEN user:=loggedOnUser
8502    IF (user=NIL) THEN RETURN FALSE
8504    IF isConfAccessAreaName(user)=FALSE
8505      IF (confNum<=StrLen(user.conferenceAccess))
8506        IF user.conferenceAccess[confNum-1]="X" THEN RETURN TRUE
8507      ENDIF
8508      RETURN FALSE
8509    ENDIF
8511    StringF(ttname,'Conf.\d',confNum)
8512  ENDPROC checkToolTypeExists(TOOLTYPE_AREA,user.conferenceAccess,ttname)
```

Two access models: a per-user string with `X` at position `confNum-1`, or (when
`conferenceAccess` holds an area name) an icon tooltype `Conf.<n>` in the named
area file. Conference-level only — message bases have no access check.

---

## 4. Fallback targets

### `internalCommandJ('')` — express.e:25113-25183 (empty-params path)

With empty params: `newConf` stays `-1` → `getInverse(-1)` returns 0
(express.e:8571: `IF(cn<1) THEN RETURN 0`) → enters the prompt branch:

```e
25142    IF (newConf<1) OR (newConf>cmds.numConf)
25143      displayScreen(SCREEN_JOINCONF)         -> node screen file 'JoinConf' (express.e:6588-6590)
25144      StringF(tempStr,'Conference Number (1-\d): ',cmds.numConf)
25145      stat:=lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)
25146      IF stat<>RESULT_SUCCESS THEN RETURN stat
25148      IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS    -> blank entry: stay put, no output
25150      newConf:=getInverse(Val(newStr))
25151    ENDIF
25153    IF newConf<1 THEN newConf:=1
25154    IF newConf>cmds.numConf THEN newConf:=cmds.numConf  -> clamp into range
25156    IF(checkConfAccess(newConf)=FALSE)
25157      aePuts('\b\nYou do not have access to the requested conference\b\n\b\n')
25158      RETURN RESULT_FAILURE
25159    ENDIF
       ...
25167    cnt:=getConfMsgBaseCount(newConf)
25169    IF (newMsgBase<1) OR (newMsgBase>cnt)               -> newMsgBase defaults to 1, so skipped here
       ...
25182    joinConf(newConf,newMsgBase,FALSE,FALSE)
```

So when `<`/`>` hits the edge, the user sees the JoinConf screen (if present)
and the prompt `Conference Number (1-N): ` (N = `cmds.numConf`, absolute count).
Note `internalCommandJ` re-runs `checkSecurity(ACS_JOIN_CONFERENCE)` (:25120),
`saveMsgPointers` (:25121) and `setEnvStat(ENV_JOIN)` (:25123) — harmless
repeats.

### `internalCommandJM('')` — express.e:25185-25237 (empty-params path)

```e
25191    IF checkSecurity(ACS_JOIN_CONFERENCE)=FALSE THEN RETURN RESULT_NOT_ALLOWED
25193    saveMsgPointers(currentConf,currentMsgBase)
25195    setEnvStat(ENV_JOIN)
25197    parseParams(params)                                  -> empty: newMsgBase stays -1
       ...
25211    cnt:=readToolTypeInt(TOOLTYPE_MSGBASE,currentConf,'NMSGBASES')
25212    IF cnt=-1
25213      aePuts('\b\nThis conference does not contain multiple message bases\b\n\b\n')
25214      RETURN RESULT_FAILURE
25215    ENDIF
25218    cnt:=getConfMsgBaseCount(currentConf)
25220    IF (newMsgBase<1) OR (newMsgBase>cnt)                -> always true for empty params
25221      IF displayScreen(SCREEN_CONF_JOINMSGBASE)=FALSE    -> conf screen dir 'JoinMsgBase' (express.e:6591-6593)
25222        displayScreen(SCREEN_JOINMSGBASE)                -> node screen dir 'JoinMsgBase' (express.e:6594-6596)
25223      ENDIF
25224      StringF(tempStr,'Message Base Number (1-\d): ',cnt)
25225      stat:=lineInput(tempStr,'',5,INPUT_TIMEOUT,newStr)
25226      IF stat<>RESULT_SUCCESS THEN RETURN stat
25228      IF StrLen(newStr)=0 THEN RETURN RESULT_SUCCESS     -> blank: stay put
25230      newMsgBase:=Val(newStr)
25231    ENDIF
25233    IF newMsgBase<1 THEN newMsgBase:=1
25234    IF newMsgBase>cnt THEN newMsgBase:=cnt               -> clamp
25236    joinConf(currentConf,newMsgBase,FALSE,FALSE)
```

(For completeness: with non-empty params containing `.`, JM delegates to
`internalCommandJ(params)` at :25203-25206 — not reachable from `<<`/`>>` since
they always pass `''`.)

---

## 5. Message base model

- `currentMsgBase` is a global, initialised to 0 (express.e:105), set to the
  joined base in `joinConf` (:4999); 1-based once in a conference.
- Count per conference — `getConfMsgBaseCount` (express.e:2048-2052):

  ```e
  2048  PROC getConfMsgBaseCount(confNum)
  2050    num:=readToolTypeInt(TOOLTYPE_MSGBASE,confNum,'NMSGBASES')
  2051    IF num=-1 THEN num:=1
  2052  ENDPROC num
  ```

  The count comes from the `NMSGBASES` tooltype of the conference's msgbase icon
  file; **absent tooltype → count 1**. (`readToolTypeInt` returns `-1` when the
  key is missing — tooltypes.e:176-181.)
- Base names/locations come from `NAME.<n>` / `LOCATION.<n>` tooltypes
  (express.e:2054-2080).

### `<<` / `>>` in a conference with exactly ONE message base

Two distinct configurations behave differently:

1. **`NMSGBASES` tooltype absent** (the normal single-base conf):
   `currentMsgBase=1`. `<<` computes 0 → `internalCommandJM('')`; `>>` computes
   2 > count(1) → `internalCommandJM('')`. JM's tooltype probe (:25211) returns
   -1 and the user sees exactly:

   ```
   '\b\nThis conference does not contain multiple message bases\b\n\b\n'   (express.e:25213)
   ```

   and the command fails (`RESULT_FAILURE`; no join happens, no join output).
2. **`NMSGBASES=1` explicitly set**: JM's probe returns 1 (not -1), so instead
   the user gets the JoinMsgBase screen + `Message Base Number (1-1): ` prompt.

---

## 6. `joinConf` — express.e:4975-5139 (what a successful `<`/`>`/`<<`/`>>` prints)

Signature (:4975): `joinConf(conf, msgBaseNum, confScan, auto, forceMailScan=FORCE_MAILSCAN_NOFORCE)`.
All four commands call it with `confScan=FALSE, auto=FALSE`, default mailscan.

Flow relevant to these commands:

```e
4982   IF (checkConfAccess(conf)=FALSE) THEN conf:=1        -> defensive re-check
4983   IF((conf<1) OR (conf>cmds.numConf)) THEN conf:=1
4984   WHILE (conf<=cmds.numConf) ANDALSO (checkConfAccess(conf)=FALSE) ... conf++ ...
4988   IF (conf>cmds.numConf)
4989     aePuts('\b\nYou do not have access to any conferences on this BBS\b\n')
4990     aePuts('Disconnecting..\b\n')
4991     reqState:=REQ_STATE_LOGOFF
4992     RETURN
4993   ENDIF
4995   IF (msgBaseNum<1) OR (msgBaseNum>getConfMsgBaseCount(conf)) THEN msgBaseNum:=1   -> clamp
4997   IF confScan=FALSE
4998     currentConf:=conf ; currentMsgBase:=msgBaseNum      (4998-4999)
5008   quietJoin:=checkToolTypeExists(TOOLTYPE_BBSCONFIG,0,'QUIET_JOIN')
5026   loadMsgPointers(conf,msgBaseNum)
5058   IF displayScreen(SCREEN_CONF_BULL)                    -> conf bulletin screen, then doPause()
5065   IF quietJoin=FALSE THEN aePuts('\b\n')
5077   IF getConfMsgBaseCount(conf)>1
5078     getMsgBaseName(conf,msgBaseNum,tempstr)
5079     StringF(string,'ESC[32mJoining Conference ESC[33m: ESC[0m \s [\s]',currentConfName,tempstr)   -> see verbatim below
5080     IF quietJoin=FALSE THEN aePuts(string)
5081     StringF(string,'\s [\s] (\d) Conference Joined',currentConfName,tempstr,conf)   -> callersLog only
5082   ELSE
5083     StringF(string,'ESC[32mJoining ConferenceESC[33m:ESC[0m \s',currentConfName)
5084     IF quietJoin=FALSE THEN aePuts(string)
5085     StringF(string,'\s (\d) Conference Joined',currentConfName,conf)                -> callersLog only
5086   ENDIF
5088   IF quietJoin=FALSE THEN aePuts('\b\n')
5089-5090  callersLog('\t'+string)
5092   IF (quietJoin=FALSE)  -> mail stats block (non-CUSTOM conf), lines 5094-5109, see verbatim strings
5119   IF (auto=FALSE) AND (forceMailScan<>FORCE_MAILSCAN_SKIP)
5120     IF ... checkMailConfScan(conf, msgBaseNum) ... callMsgFuncs(MAIL_SCAN,...) ; saveMsgPointers (5122-5126)
5130   IF (auto=FALSE) AND (confScan=FALSE)
5135     loggedOnUser.confRJoin:=conf ; loggedOnUser.msgBaseRJoin:=msgBaseNum            (rejoin persisted)
5137     createNodeUserFiles()
```

Note: the `Joining Conference:` line includes ` [<msgBaseName>]` **only when the
target conference has >1 message base** (:5077). The `(\d) Conference Joined`
variants go to the callers log, not the wire. `FORCE_MAILSCAN_NOFORCE=0`
(axenums.e:40), so a post-join mail scan runs iff `checkMailConfScan` says the
user has that conf/base flagged for scanning (runtime state).

---

## 7. Verbatim strings

`ESC` = literal 0x1B byte in the source file.

| # | String (verbatim) | Source line | Emitted when |
|---|---|---|---|
| 1 | `'\b\nCommand requires higher access.\b\n'` | express.e:3038 | `<` or `>` (or JM fallback) without `ACS_JOIN_CONFERENCE`, via `higherAccess()` from express.e:28390 |
| 2 | `'\b\nNo such command!!  Use ''?'' for command list.\b\n\b\n'` | express.e:28387 | unknown token (two literal spaces after `!!`; `''` is an escaped single quote) |
| 3 | `'Conference Number (1-\d): '` | express.e:25144 | J interactive prompt (fallback for `<`/`>` at the edge); `\d` = `cmds.numConf` |
| 4 | `'\b\nYou do not have access to the requested conference\b\n\b\n'` | express.e:25157 | J prompt: entered conf fails `checkConfAccess` |
| 5 | `'\b\nThis conference does not contain multiple message bases\b\n\b\n'` | express.e:25213 | JM fallback (from `<<`/`>>`) when `NMSGBASES` tooltype absent |
| 6 | `'Message Base Number (1-\d): '` | express.e:25224 (same at :25173 in J) | JM interactive prompt (fallback for `<<`/`>>` at the edge); `\d` = `getConfMsgBaseCount(currentConf)` |
| 7 | `'\b\nYou do not have access to any conferences on this BBS\b\n'` then `'Disconnecting..\b\n'` | express.e:4989-4990 | joinConf defensive walk finds no accessible conf (not normally reachable from `<`/`>`) |
| 8 | `'ESC[32mJoining ConferenceESC[33m:ESC[0m \s [\s]'` | express.e:5079 | join output, multi-msgbase conf; `\s`=conf name, `[\s]`=msgbase name; suppressed by QUIET_JOIN |
| 9 | `'ESC[32mJoining ConferenceESC[33m:ESC[0m \s'` | express.e:5083 | join output, single-msgbase conf |
| 10 | `'\s [\s] (\d) Conference Joined'` / `'\s (\d) Conference Joined'` | express.e:5081 / 5085 | **callersLog only**, not the wire |
| 11 | `'ESC[32mMessages range from ESC[33m( ESC[0m\d ESC[32m- ESC[0m\d ESC[33m)ESC[0m\b\n'` | express.e:5096-5097 | join stats when `mailStat.lowestKey>1`; args `lowestKey`, `highMsgNum-1` |
| 12 | `'\b\nESC[32mTotal messages           ESC[33m:ESC[0m \d\b\n'` | express.e:5099 | join stats otherwise; arg `highMsgNum-1` |
| 13 | `'\b\nESC[32mLast message auto scannedESC[33m:ESC[0m \d\b\n'` | express.e:5105 | join stats; arg `lastNewReadConf-1` (clamped to ≥1 via :5103-5104) |
| 14 | `'ESC[32mLast message read        ESC[33m:ESC[0m \d\b\n'` | express.e:5108 | join stats; arg `lastMsgReadConf` |
| 15 | `'\b\n'` | express.e:5065 and 5088 | blank line before and after the Joining line (suppressed by QUIET_JOIN) |
| 16 | node status `'JOINING CONF'` | express.e:24326 | ACP/WHO node status while in ENV_JOIN — not wire output of these commands |

Screens displayed (file lookups, content is sysop-supplied):
- `SCREEN_JOINCONF` → `<nodeScreenDir>JoinConf` (express.e:6588-6590) — before prompt 3.
- `SCREEN_CONF_JOINMSGBASE` → `<confScreenDir>JoinMsgBase` (express.e:6591-6593), falling back to `SCREEN_JOINMSGBASE` → `<nodeScreenDir>JoinMsgBase` (express.e:6594-6596) — before prompt 6.
- `SCREEN_CONF_BULL` inside joinConf (:5058) followed by `doPause()` whose prompt is `'\b\nESC[32m(ESC[33mPauseESC[32m)ESC[34m...ESC[32mSpace To Resume ESC[33m: ESC[0m'` (express.e:5144).

---

## 8. Control flow summary (pseudocode)

```
"<"  (express.e:24529)                         ">"  (express.e:24548)
  require ACS_JOIN_CONFERENCE else NOT_ALLOWED   require ACS_JOIN_CONFERENCE else NOT_ALLOWED
  saveMsgPointers(curConf,curBase)               saveMsgPointers(curConf,curBase)
  setEnvStat(ENV_JOIN)                           setEnvStat(ENV_JOIN)
  n := curConf-1                                 n := curConf+1
  while n>0 and !confAccess(n): n--              while n<=numConf and !confAccess(n): n++
  if n<1:  J-interactive-prompt('')              if n>numConf:  J-interactive-prompt('')
  else:    joinConf(n, msgbase=1, FALSE,FALSE)   else:          joinConf(n, msgbase=1, FALSE,FALSE)
  return SUCCESS                                 return SUCCESS

"<<" (express.e:24566)                         ">>" (express.e:24580)
  (NO security check here)                       (NO security check here)
  saveMsgPointers; setEnvStat(ENV_JOIN)          saveMsgPointers; setEnvStat(ENV_JOIN)
  b := curBase-1                                 b := curBase+1
  if b<1:  JM-interactive-prompt('')             if b>msgBaseCount(curConf): JM-interactive-prompt('')
  else:    joinConf(curConf, b, FALSE,FALSE)     else: joinConf(curConf, b, FALSE,FALSE)
  return SUCCESS                                 return SUCCESS

JM-interactive-prompt('') (express.e:25185):
  require ACS_JOIN_CONFERENCE
  if NMSGBASES tooltype missing: print string #5; FAILURE
  show JoinMsgBase screen; prompt "Message Base Number (1-cnt): "
  blank → SUCCESS (stay); else clamp 1..cnt; joinConf(curConf, b, FALSE, FALSE)

J-interactive-prompt('') (express.e:25113):
  require ACS_JOIN_CONFERENCE
  show JoinConf screen; prompt "Conference Number (1-numConf): "
  blank → SUCCESS (stay); else getInverse(); clamp 1..numConf
  !confAccess → print string #4; FAILURE
  joinConf(conf, 1, FALSE, FALSE)
```

Parity-relevant behavioural points:

1. No wraparound in any of the four commands; the edge always lands in the
   interactive J/JM prompt.
2. `<`/`>` always land on **message base 1** of the neighbouring conference,
   regardless of the base previously used in that conference.
3. `<`/`>` skip inaccessible conferences transparently (no message per skip).
4. `<<`/`>>` themselves perform **no security or access checks**; only the JM
   fallback path enforces `ACS_JOIN_CONFERENCE`, and there is no per-msgbase
   access concept at all.
5. `<`/`>` walk **absolute** conference numbers; relative-conference numbering
   (`TOGGLES_CONFRELATIVE`, `getInverse`/`relConf`, express.e:8558-8581) only
   affects the J prompt's number parsing, not the neighbour walk.
6. Edge fallback in a single-msgbase conf (`NMSGBASES` absent) yields the
   "does not contain multiple message bases" failure rather than a prompt.
7. Successful joins persist `confRJoin`/`msgBaseRJoin` (express.e:5135-5136).

---

## 9. Open questions (need live observation / runtime config)

1. **QUIET_JOIN tooltype**: when `BBSCONFIG` has `QUIET_JOIN`
   (express.e:5008), the Joining line, surrounding blank lines and stats block
   are all suppressed — is it set on the reference BBS we compare against?
2. **CUSTOM conferences** (express.e:5028, 5093, 5110-5112): a conf with the
   `CUSTOM` tooltype skips `getMailStatFile` and replaces the stats block with
   `customMsgbaseCmd(MAIL_STATS,...)` output. Unknown whether any reference
   conf is CUSTOM.
3. **Post-join mail scan**: whether `checkMailConfScan(conf,msgBaseNum)`
   triggers a `MAIL_SCAN` after `<`/`>`/`<<`/`>>` (express.e:5119-5128) depends
   on the user's per-conf scan flags — observe live what a scan emits.
4. **`SCREEN_CONF_BULL` / `JoinConf` / `JoinMsgBase` screen files**: presence
   and content are BBS-config; the doPause after a conf bulletin (:5058-5061)
   only happens if the screen exists.
5. **Custom door shadowing**: `runSysCommand`/`runBbsCommand` are tried before
   internal dispatch (express.e:28248-28254); a node could define a `<` door.
   Assume absent unless the reference BBS shows otherwise.
6. **`NMSGBASES=1` explicitly set** (vs absent): produces the
   `Message Base Number (1-1): ` prompt path instead of the failure string —
   worth a live capture if the reference BBS has such a conf.
7. **Exact mail-stats numbers** (`lowestKey`, `highMsgNum`, `lastMsgReadConf`,
   `lastNewReadConf`) depend on the msgbase contents and `getMailStatFile`
   semantics (express.e:5029-5049) — pin with live captures.
8. **`getInverse` with relative conferencing OFF** simply returns the typed
   number (express.e:8572); with it ON, the J prompt's `(1-\d)` upper bound
   still shows absolute `cmds.numConf` (express.e:25144) — verify the
   reference BBS's TOGGLES_CONFRELATIVE setting before asserting prompt parity.
