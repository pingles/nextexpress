#!/usr/bin/env python3
"""Capture AmiExpress reference behaviour for Tier C (conference navigation).

Scenarios: J no-arg interactive prompt (blank / valid / invalid / out-of-range),
< / > prev-next conference (including edges), << / >> msgbase siblings,
JM no-arg + numeric + out-of-range + dotted args.

Every session ends with a clean `G Y` logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bbsdrive import BBS, strip_iac, render  # noqa: E402
from ae_session import connect_until_node  # noqa: E402

HOST, PORT = "127.0.0.1", 6023
MENU_SENTINEL = b"mins. left): "
PAUSE_SENTINEL = b"Space To Resume"

LOG = []


def emit(label, sent, clean, status):
    LOG.append(f"\n@@@@@ {label} @@@@@ [{status}]")
    if sent is not None:
        LOG.append(f">>> SENT {sent!r}")
    LOG.append("----- RENDER -----")
    LOG.append(render(clean))
    LOG.append("----- REPR -----")
    LOG.append(repr(clean))


def read_until_any(bbs, patterns, maxwait=40):
    """Read until any of `patterns` appears in the IAC-stripped stream.
    Returns (clean, matched_pattern_or_None)."""
    import time
    chunks = bytearray()
    start = time.time()
    bbs.sock.settimeout(0.25)
    while time.time() - start < maxwait:
        try:
            data = bbs.sock.recv(4096)
            if data == b"":
                break
            chunks += data
            bbs.all_raw += data
        except OSError:
            pass
        clean, _ = strip_iac(bytes(chunks))
        for p in patterns:
            if p in clean:
                return clean, p
    clean, _ = strip_iac(bytes(chunks))
    return clean, None


def to_menu(bbs, label, send=None, maxwait=45):
    """Send `send`, then read to the next menu prompt, answering any
    (Pause)...Space To Resume gates with a space. Captures everything."""
    if send is not None:
        bbs.send(send)
    collected = b""
    status = "TIMEOUT"
    for _ in range(8):
        clean, hit = read_until_any(bbs, [MENU_SENTINEL, PAUSE_SENTINEL], maxwait=maxwait)
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness sends SPACE>>>"
            bbs.send(b" ")
            continue
        break
    emit(label, send, collected, status)
    return collected


def to_pattern(bbs, label, send, pattern, maxwait=40):
    if send is not None:
        bbs.send(send)
    clean, hit = read_until_any(bbs, [pattern, PAUSE_SENTINEL, MENU_SENTINEL], maxwait=maxwait)
    status = "MATCHED" if hit == pattern else f"GOT {hit!r}"
    emit(label, send, clean, status)
    return clean, hit


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierc.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        # ---- login ----
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=40)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN (to first menu prompt)", b"sysop\r", maxwait=90)

        # ---- C2: J no-arg interactive prompt ----
        # The auto-rejoin lands in conference 2 (Amiga).
        to_pattern(bbs, "C2: J no-arg -> prompt?", b"J\r", b": ", maxwait=30)
        to_menu(bbs, "C2: blank input at J prompt", b"\r")

        to_pattern(bbs, "C2: J no-arg again", b"J\r", b": ", maxwait=30)
        to_menu(bbs, "C2: enter 1 at J prompt (join New Users)", b"1\r")

        to_pattern(bbs, "C2: J no-arg (for invalid text)", b"J\r", b": ", maxwait=30)
        to_menu(bbs, "C2: enter abc at J prompt", b"abc\r")

        to_pattern(bbs, "C2: J no-arg (for out-of-range)", b"J\r", b": ", maxwait=30)
        to_menu(bbs, "C2: enter 99 at J prompt", b"99\r")

        to_pattern(bbs, "C2: J no-arg (for zero)", b"J\r", b": ", maxwait=30)
        to_menu(bbs, "C2: enter 0 at J prompt", b"0\r")

        # ---- C3: < / > prev-next conference ----
        # State check: we should be in conf 1 (New Users) after the J 1 above.
        to_menu(bbs, "C3: > from conf 1 (expect move to 2)", b">\r")
        to_menu(bbs, "C3: > from conf 2 (upper edge)", b">\r")
        to_menu(bbs, "C3: < from wherever (expect move down)", b"<\r")
        to_menu(bbs, "C3: < again (lower edge)", b"<\r")
        to_menu(bbs, "C3: < once more (confirm edge behaviour)", b"<\r")

        # ---- C4b: << / >> msgbase siblings (single-base conferences) ----
        to_menu(bbs, "C4b: >> (next msgbase, single-base conf)", b">>\r")
        to_menu(bbs, "C4b: << (prev msgbase, single-base conf)", b"<<\r")

        # ---- C4a/C4b: JM ----
        to_menu(bbs, "C4a: JM 1 (explicit msgbase)", b"JM 1\r")
        to_menu(bbs, "C4a: JM 9 (out of range)", b"JM 9\r")
        to_menu(bbs, "C4a: JM abc (invalid)", b"JM abc\r")
        # no-arg: may prompt — go pattern-first, then answer
        clean, hit = to_pattern(bbs, "C4b: JM no-arg -> prompt?", b"JM\r", b": ", maxwait=30)
        if hit not in (MENU_SENTINEL,):
            to_menu(bbs, "C4b: blank input at JM prompt", b"\r")
            clean2, hit2 = to_pattern(bbs, "C4b: JM no-arg again", b"JM\r", b": ", maxwait=30)
            if hit2 not in (MENU_SENTINEL,):
                to_menu(bbs, "C4b: enter 1 at JM prompt", b"1\r")

        # ---- dotted args ----
        to_menu(bbs, "dotted: J 1.1", b"J 1.1\r")
        to_menu(bbs, "dotted: J 2.1", b"J 2.1\r")
        to_menu(bbs, "dotted: JM 1.1", b"JM 1.1\r")

    finally:
        # clean logoff — MANDATORY for the FS-UAE node
        try:
            bbs.send(b"G Y\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.5, maxwait=12)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
                low = clean.lower()
                if b"y/n" in low or b"sure" in low:
                    bbs.send(b"Y\r")
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {out_path}")


if __name__ == "__main__":
    main()
