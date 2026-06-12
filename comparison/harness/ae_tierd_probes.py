#!/usr/bin/env python3
"""Tier D probes — the three uncaptured AquaScan corners (design
2026-06-12 §6.1):

  P1: held lone `n` at More?, then bare CR  -> does n+Enter Quit?
  P2: bare LF at a fresh More?              -> LF as a keypress?
  P3: flag prompt fed one byte at a time    -> per-keystroke echo?

Each step logs the EXACT bytes sent and a per-step idle snapshot of the
bytes received, so echo timing is observable (the gap that caused the
D2 echo defect). Every session ends with a clean `G Y` logoff (FS-UAE
node-spin hazard — see ae_tierd_aquascan3.py).
"""
import sys
import os
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, HOST, PORT,
    MENU_SENTINEL, read_until_any, emit,
)
from ae_session import connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402

MORE = b"uit:"      # ...(Q)uit:
FLAG = b"to flag:"  # File name(s) to flag:


def snapshot(bbs, label, data, collected, settle=3):
    """Send `data` raw, then read everything that arrives within `settle`
    seconds of idle.

    Uses an impossible sentinel so ``read_until_any`` runs to its timeout
    and returns whatever arrived, which is exactly the idle-snapshot we want.
    Returns the received bytes and appends a labelled record to ``collected``.
    """
    bbs.send(data)
    got, _ = read_until_any(bbs, [b"\xde\xad\xbe\xef"], maxwait=settle)
    collected.append(
        b"<<<sent " + repr(data).encode() + b" got>>>" + got
        + b"<<<end " + label.encode() + b">>>",
    )
    return got


def to_more(bbs, collected):
    """Send ``F 1\\r`` and block until the More? pager prompt appears.

    Asserts the pager prompt is reached and appends raw received bytes to
    ``collected``.
    """
    bbs.send(b"F 1\r")
    got, hit = read_until_any(bbs, [MORE], maxwait=45)
    collected.append(got)
    assert hit == MORE, "never reached More? prompt"


def recover(bbs, collected):
    """Best-effort escape back to the menu after a probe leaves an unknown state.

    Tries ``Q``, bare CR, and ``q\\r`` in turn; stops as soon as the menu
    sentinel appears.  Raises ``RuntimeError`` if all attempts fail.
    """
    for esc in (b"Q", b"\r", b"q\r"):
        bbs.send(esc)
        got, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=20)
        collected.append(b"<<<recovery " + repr(esc).encode() + b">>>" + got)
        if hit == MENU_SENTINEL:
            return
    raise RuntimeError("could not recover to menu")


def main():
    """Run all three Tier D probes and write a transcript to ``out_path``."""
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_probes.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # ------------------------------------------------------------------
        # P1: lone n at More?, idle snapshot, then bare CR.
        # Question: does n + Enter quit the pager to the menu?
        # ------------------------------------------------------------------
        c = []
        to_more(bbs, c)
        snapshot(bbs, "P1 lone n", b"n", c)
        got = snapshot(bbs, "P1 n then CR", b"\r", c, settle=5)
        emit("P1: held n + Enter", b"n,\\r", b"".join(c),
             "MENU" if MENU_SENTINEL in got else "STILL IN PAGER")
        if MENU_SENTINEL not in got:
            recover(bbs, c)

        # ------------------------------------------------------------------
        # P2: bare LF at a fresh More?.
        # Question: is LF treated as a keypress (like CR) or ignored?
        # ------------------------------------------------------------------
        c = []
        to_more(bbs, c)
        got = snapshot(bbs, "P2 bare LF", b"\n", c, settle=5)
        emit("P2: bare LF at More?", b"\\n", b"".join(c),
             "MENU" if MENU_SENTINEL in got else "PAGER/STREAMED")
        if MENU_SENTINEL not in got:
            recover(bbs, c)

        # ------------------------------------------------------------------
        # P3: flag entry typed one byte at a time.
        # Question: does each byte echo back individually as typed?
        # ------------------------------------------------------------------
        c = []
        to_more(bbs, c)
        bbs.send(b"F")
        got, hit = read_until_any(bbs, [FLAG], maxwait=20)
        c.append(got)
        assert hit == FLAG, "flag prompt never appeared"
        for byte in b"TERMV48":
            snapshot(bbs, "P3 byte %c" % byte, bytes([byte]), c, settle=2)
            time.sleep(0.3)
        snapshot(bbs, "P3 finish", b".LHA\r", c, settle=5)
        emit("P3: flag entry per-byte echo", b"T,E,R,M,V,4,8,.LHA\\r",
             b"".join(c), "DONE")
        recover(bbs, c)

    finally:
        # Clean logoff — MANDATORY: abrupt close causes FS-UAE node spin-wait.
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
