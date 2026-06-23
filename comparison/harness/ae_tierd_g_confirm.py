#!/usr/bin/env python3
"""Tier D5 capture — plain-G checkFlagged confirm (genuine internalCommandG).

`G` is NOT door-shadowed, so plain `G` runs internalCommandG (express.e:25045).
With files flagged, it calls checkFlagged() (express.e:12667):

    \\b\\nYou have flagged files still not downloaded.\\b\\nDo you leave without them?

then yesNo(2) (express.e:2129) — a SINGLE-KEY readChar that prints its own ANSI
`(y/N)? ` suffix, echoes `Yes`/`No`, CR defaults to N. N (stay) -> internal
`\\b\\n` + return to menu; Y (leave) -> saveFlagged (** AutoSaving File Flags **)
-> logoff.

We flag a real file via the AquaScan F pager (keeping it — Q at More?, not C),
return to the menu, then drive plain G through both branches. The Y branch
doubles as the mandatory clean logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, read_until_any, emit,
    HOST, PORT, MENU_SENTINEL, PAUSE_SENTINEL,
)
from ae_session import connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402

MORE_PROMPT = b"uit:"        # AquaScan More? ...(Q)uit:
FLAG_PROMPT = b"to flag:"    # File name(s) to flag:
CONFIRM_TAIL = b"them? "     # ...Do you leave without them?  (before yesNo suffix)
DOWNLOADED = b"not downloaded"
AUTOSAVE = b"AutoSaving"


def flag_keep(bbs, send=b"F 2\r", fname=b"MYDEMO.DMS\r"):
    """Drive AquaScan: list a dir, at the first More? open the flag prompt,
    flag `fname`, then Q to quit the pager KEEPING the flag (not C=clear)."""
    bbs.send(send)
    collected = b""
    flagged = False
    status = "RAN OUT OF ROUNDS"
    for _ in range(40):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG_PROMPT, MORE_PROMPT, PAUSE_SENTINEL],
            maxwait=45)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU (flag kept)" if flagged else "MENU (NOT flagged!)"
            break
        if hit == FLAG_PROMPT:
            collected += b"<<<flag: %s>>>" % fname
            bbs.send(fname)
            flagged = True
            continue
        if hit == MORE_PROMPT:
            ans = b"Q" if flagged else b"F"   # F opens flag prompt; Q quits keeping flags
            collected += b"<<<More?: %s>>>" % ans
            bbs.send(ans)
            continue
        if hit == PAUSE_SENTINEL:
            collected += b"<<<SPACE>>>"
            bbs.send(b" ")
            continue
        status = "TIMEOUT"
        break
    emit("flag %s via F-pager (keep)" % fname.strip().decode(), send, collected, status)
    return collected


def capture_confirm(bbs, label):
    """Send plain G, read to quiet (the yesNo prompt waits on a single key).
    Returns (clean, kind) where kind in {confirm, loggedoff, menu, unknown}."""
    bbs.send(b"G\r")
    clean, hit = read_until_any(
        bbs, [CONFIRM_TAIL, AUTOSAVE, b"Goodbye", MENU_SENTINEL], maxwait=30)
    # If the confirm tail matched, read a touch more to capture the yesNo suffix.
    if hit == CONFIRM_TAIL:
        clean += bbs.read_idle(idle=1.0, maxwait=6)
        kind = "confirm"
    elif hit in (AUTOSAVE, b"Goodbye"):
        clean += bbs.read_idle(idle=1.5, maxwait=12)
        kind = "loggedoff"
    elif hit == MENU_SENTINEL:
        kind = "menu"
    else:
        kind = "unknown"
    emit(label, b"G\r", clean, "kind=%s" % kind)
    return clean, kind


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_g_confirm.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        login = to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        emit("BANNER CHECK", None,
             b"** Flagged File(s) Exist ** PRESENT at login"
             if b"Flagged File(s) Exist" in login
             else b"(no saved-flag banner at login)", "info")

        to_menu(bbs, "J 2 (Amiga, seeded files)", b"J 2\r")
        flag_keep(bbs)

        # --- plain G, branch 1: answer N (stay -> back to menu) ---
        clean, kind = capture_confirm(bbs, "PLAIN G (#1) -> confirm prompt")
        if kind == "confirm":
            stay = to_menu(bbs, "confirm -> N (single key, expect stay->menu)", b"N")
            # --- plain G, branch 2: answer Y (leave -> saveFlagged -> logoff) ---
            clean2, kind2 = capture_confirm(bbs, "PLAIN G (#2) -> confirm prompt")
            if kind2 == "confirm":
                bbs.send(b"Y")
                logoff = bbs.read_idle(idle=2.0, maxwait=20)
                emit("confirm -> Y (single key, leave -> logoff)", b"Y", logoff, "logoff")
            else:
                emit("PLAIN G (#2) unexpected", None, clean2, "kind=%s" % kind2)
        else:
            emit("PLAIN G (#1) unexpected — investigate flag path", None, clean, "kind=%s" % kind)
    finally:
        # Safety net: if still connected, force a clean logoff.
        try:
            bbs.send(b"G Y\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.5, maxwait=10)
                LOG.append("\n@@@@@ SAFETY LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print("wrote", out_path)


if __name__ == "__main__":
    main()
