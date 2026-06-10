#!/usr/bin/env python3
"""Tier D capture — stock internal F/FR (door icons moved aside).

Run with F.info/FR.info/N.info moved out of BBS:Commands/BBSCmd/ (and the
board restarted — the disk-object cache keeps serving moved icons).

Stock prompts (from the first stock pass, ae_tierd_stock.txt):
  ListPause -> `(Pause)...(f)lags, More(Y/n/ns)? ` — the listing pager
               (lineCount machinery). REJECTS unknown input and redraws
               (ESC[A ESC[K + re-prompt). Has an (f)lags option.
  Dirs      -> `Directories: (1-N), (A)ll, (U)pload, (H)old, (Enter)=none? `
               (getDirSpan, express.e:26864; H only with hold access)
  Wire quirk: DIR rows stream as LF-CR (displayIt2 appends CR after the
  file's LF) — pinned by the first pass.

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
    MENU_SENTINEL, PAUSE_SENTINEL, read_until_any, emit,
)
from bbsdrive import render  # noqa: E402

LISTPAUSE = b"lags, More"     # (Pause)...(f)lags, More(Y/n/ns)?
DIRS = b"=none? "             # getDirSpan prompt (lowercase none)
FLAGIN = b"to flag"           # speculative: internal flag sub-prompt

DEFAULTS = {LISTPAUSE: b"Y", DIRS: b"\r", FLAGIN: b"\r"}


def scenario(bbs, label, send, pause=(), dirs=(), flagin=(),
             maxrounds=40, maxwait=45):
    queues = {LISTPAUSE: list(pause), DIRS: list(dirs), FLAGIN: list(flagin)}
    bbs.send(send)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAGIN, DIRS, LISTPAUSE, PAUSE_SENTINEL],
            maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness: SPACE>>>"
            bbs.send(b" ")
            continue
        if hit is None:
            status = "TIMEOUT (no sentinel)"
            break
        q = queues.get(hit)
        ans = q.pop(0) if q else DEFAULTS[hit]
        collected += b"<<<harness answers %s: %s>>>" % (hit, ans)
        bbs.send(ans)
    if status != "MENU":
        for esc in (b"n", b"Q", b"\r"):
            bbs.send(esc)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=20)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_stock2.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        scenario(bbs, "T1: F 1, Y through to end", b"F 1\r")
        scenario(bbs, "T2: F 1, n at first pause (stop)", b"F 1\r",
                 pause=[b"n"])
        scenario(bbs, "T3: F 1, ns at first pause (non-stop rest)",
                 b"F 1\r", pause=[b"ns"])
        scenario(bbs, "T4: F 1, f at pause (flags option)", b"F 1\r",
                 pause=[b"f", b"n"], flagin=[b"TERMV48.LHA\r"])
        scenario(bbs, "T5: F 2 (upload dir, T: copy)", b"F 2\r")
        scenario(bbs, "T6: F A (all-dirs walk)", b"F A\r")
        scenario(bbs, "T7: F U (upload shortcut)", b"F U\r")
        scenario(bbs, "T8: F H (hold, file likely missing)", b"F H\r")
        scenario(bbs, "T9: F 99 (No such directory.)", b"F 99\r")
        scenario(bbs, "T10: F 1 NS (pause suppressed)", b"F 1 NS\r")
        scenario(bbs, "T11: bare F -> answer 1", b"F\r", dirs=[b"1\r"])
        scenario(bbs, "T12: bare F -> Enter (=none abort)", b"F\r",
                 dirs=[b"\r"])
        scenario(bbs, "T13: FR 1 (reverse scan)", b"FR 1\r")
        scenario(bbs, "T14: FR A (reverse all)", b"FR A\r")
        scenario(bbs, "T15: N (internal new-files semantics)", b"N\r")

        to_menu(bbs, "E1: J 1 (empty Dir1 conf)", b"J 1\r")
        scenario(bbs, "E2: F 1 (empty DIR file)", b"F 1\r")
        scenario(bbs, "E3: bare F single-dir conf -> Enter", b"F\r",
                 dirs=[b"\r"])
        to_menu(bbs, "restore rejoin target: J 2", b"J 2\r")
    finally:
        try:
            bbs.send(b"G Y\r")
            for _ in range(8):
                clean = bbs.read_idle(idle=1.5, maxwait=12)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
