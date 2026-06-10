#!/usr/bin/env python3
"""Tier D capture — AquaScan door behaviour (the board-as-shipped F/FR/N).

The stock deployment ships AquaScan v1.0 door icons shadowing F, FR, N (and
CS, NSU, SCAN, SENT) in BBS:Commands/BBSCmd/ — processCommand
(express.e:28229) runs BBS commands before internal ones, so the reference
experience for these tokens IS AquaScan, not internalCommandF. NextExpress
implements the AquaScan experience (user decision 2026-06-10); these are the
primary parity captures. Stock internal captures (icons moved aside) are
taken separately for the difference record.

Board fixture: Conf02 NDIRS=2, Dir1 = 28 seeded entries (authentic upload-
writer row format), Dir2 = 3 entries, Conf01/Dir1 empty.

AquaScan pager notes (from the first accidental capture): `More? (Y/n/ns),
(C)lear, (F/R) Flag, (?) Help, (Q)uit:` reads single-key hotkeys; unknown
keys continue; `F` opens a line-read `File name(s) to flag:` prompt.

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, connect_until_node, HOST, PORT, MENU_SENTINEL,
    PAUSE_SENTINEL, read_until_any, emit,
)
from bbsdrive import render  # noqa: E402

MORE_PROMPT = b"uit:"            # ...(Q)uit:  (ANSI between letters)
FLAG_PROMPT = b"to flag:"        # File name(s) to flag:


def drive_pager(bbs, label, send, more_answers, flag_answers=None,
                maxrounds=40, maxwait=45):
    """Send a command, then drive AquaScan's pager: feed `more_answers` at
    each More? prompt (default b"Y" when exhausted), `flag_answers` lines at
    each flag prompt (default just CR), space at (Pause), until the menu
    prompt returns. Captures the full byte stream."""
    bbs.send(send)
    more_answers = list(more_answers)
    flag_answers = list(flag_answers or [])
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG_PROMPT, MORE_PROMPT, PAUSE_SENTINEL],
            maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == FLAG_PROMPT:
            ans = flag_answers.pop(0) if flag_answers else b"\r"
            collected += b"<<<harness answers flag prompt: %s>>>" % ans
            bbs.send(ans)
            continue
        if hit == MORE_PROMPT:
            ans = more_answers.pop(0) if more_answers else b"Y"
            collected += b"<<<harness answers More?: %s>>>" % ans
            bbs.send(ans)
            continue
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness sends SPACE>>>"
            bbs.send(b" ")
            continue
        status = "TIMEOUT (no sentinel)"
        break
    emit(label, send, collected, status)
    return collected


def probe(bbs, label, send, maxwait=20):
    """Run an unknown-UI door command: capture idle output, then try to get
    back to the menu with gentle escapes (CR, Q, CR)."""
    bbs.send(send)
    collected = b""
    status = "TIMEOUT"
    escapes = [b"\r", b"Q", b"\r", b"q\r", b" "]
    for i in range(8):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, MORE_PROMPT, FLAG_PROMPT, PAUSE_SENTINEL],
            maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == MORE_PROMPT:
            collected += b"<<<harness answers More?: Q>>>"
            bbs.send(b"Q")
            continue
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness sends SPACE>>>"
            bbs.send(b" ")
            continue
        if hit == FLAG_PROMPT:
            collected += b"<<<harness answers flag prompt: CR>>>"
            bbs.send(b"\r")
            continue
        esc = escapes[min(i, len(escapes) - 1)]
        collected += b"<<<harness escape: %s>>>" % esc
        bbs.send(esc)
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_aquascan_full.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        from ae_tierc import to_pattern
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        drive_pager(bbs, "A1: bare F, stop at first More?", b"F\r", [b"n"])
        drive_pager(bbs, "A2: F 1, help (?) then page (Y) then quit (Q)",
                    b"F 1\r", [b"?", b"Y", b"Q"])
        drive_pager(bbs, "A3: F 1, ns at first More? (non-stop?)",
                    b"F 1\r", [b"ns", b"n", b"n"])
        drive_pager(bbs, "A4: F 1, flag TERMV48.LHA then Clear then quit",
                    b"F 1\r", [b"F", b"C", b"Q"],
                    flag_answers=[b"TERMV48.LHA\r"])
        drive_pager(bbs, "A5: F 2 (second dir)", b"F 2\r", [b"n"])
        drive_pager(bbs, "A6: F A (all dirs?)", b"F A\r",
                    [b"Y", b"Y", b"Y", b"Q"])
        drive_pager(bbs, "A7: F 99 (out of range)", b"F 99\r", [b"n"])
        drive_pager(bbs, "A8: F U (upload-dir token?)", b"F U\r", [b"n"])
        drive_pager(bbs, "A9: FR 1 (reverse?)", b"FR 1\r", [b"Y", b"n"])
        drive_pager(bbs, "A10: N (new files door)", b"N\r", [b"n", b"n"])

        probe(bbs, "P1: CS (door)", b"CS\r")
        probe(bbs, "P2: SCAN (door)", b"SCAN\r")
        probe(bbs, "P3: SENT (door)", b"SENT\r")
        probe(bbs, "P4: NSU (door)", b"NSU\r")

        to_menu(bbs, "E1: J 1 (empty Dir1 conf)", b"J 1\r")
        drive_pager(bbs, "E2: F 1 via AquaScan, empty dir", b"F 1\r", [b"n"])
        drive_pager(bbs, "E3: bare F, empty dir", b"F\r", [b"n"])

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
