#!/usr/bin/env python3
"""Replay the Tier C battery against a running NextExpress binary and capture
the wire bytes for side-by-side comparison with the AmiExpress reference
captures (ae_tierc*.txt).

Usage: python3 rust_tierc.py [PORT] [OUT]
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bbsdrive import BBS, strip_iac, render  # noqa: E402

HOST = "127.0.0.1"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2323
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rust_tierc.txt"

bbs = BBS(HOST, PORT, idle=0.5, maxwait=6)
log = []


def cap(label, send=None, idle=0.5, maxwait=6):
    if send is not None:
        bbs.send(send)
    data = bbs.read_idle(idle=idle, maxwait=maxwait)
    clean, _ = strip_iac(data)
    log.append(f"\n@@@@@ {label} @@@@@")
    if send is not None:
        log.append(f">>> SENT {send!r}")
    log.append("----- RENDER -----")
    log.append(render(clean))
    log.append("----- REPR -----")
    log.append(repr(clean))
    return clean


# --- login (NextExpress asks ANSI Graphics (Y/n)? first) ---
cap("CONNECT / banner", None, maxwait=6)
cap("graphics Y", b"Y\r")
cap("LOGIN name", b"sysop\r")
post = cap("LOGIN password -> menu", b"sysop\r", maxwait=8)
if b"read it now" in post:
    cap("decline logon read-it-now", b"n\r", maxwait=8)

# --- C2: J interactive prompt ---
cap("C2: J no-arg -> prompt", b"J\r")
cap("C2: blank abort", b"\r")
cap("C2: J no-arg again", b"J\r")
cap("C2: 1 at prompt (join Main)", b"1\r")
cap("C2: J no-arg (clamp high)", b"J\r")
cap("C2: 99 at prompt (clamp -> 2)", b"99\r")
cap("C2: J no-arg (clamp low)", b"J\r")
cap("C2: 0 at prompt (clamp -> 1)", b"0\r")
cap("C2: J no-arg (non-numeric)", b"J\r")
cap("C2: abc at prompt (-> 1)", b"abc\r")
cap("C2: J 99 direct -> prompt", b"J 99\r")
cap("C2: blank abort", b"\r")
cap("C2: J 0 direct -> prompt", b"J 0\r")
cap("C2: blank abort", b"\r")
cap("C2: J abc direct -> prompt", b"J abc\r")
cap("C2: blank abort", b"\r")
cap("C2: J -1 direct -> prompt", b"J -1\r")
cap("C2: blank abort", b"\r")
cap("C2: J +2 direct -> prompt (Val rejects +)", b"J +2\r")
cap("C2: blank abort", b"\r")
cap("C2: J 2abc direct -> joins 2 (Val prefix)", b"J 2abc\r")
cap("reset: J 1", b"J 1\r")

# --- C3: < / > ---
cap("C3: > from 1 (join 2)", b">\r")
cap("C3: > from 2 (edge -> prompt)", b">\r")
cap("C3: blank abort (stay 2)", b"\r")
cap("C3: < from 2 (join 1)", b"<\r")
cap("C3: < from 1 (edge -> prompt)", b"<\r")
cap("C3: blank abort (stay 1)", b"\r")

# --- C4: << >> JM on single-base conferences ---
cap("C4: >> (single base)", b">>\r")
cap("C4: << (single base)", b"<<\r")
cap("C4: JM (single base)", b"JM\r")
cap("C4: JM 1 (single base)", b"JM 1\r")
cap("C4: JM 9 (single base)", b"JM 9\r")
cap("C4: JM abc (single base)", b"JM abc\r")

# --- dotted / two-token ---
cap("C4: JM 1.1 (dotted -> J)", b"JM 1.1\r")
cap("C4: J 2.1 (dotted)", b"J 2.1\r")
cap("C4: J 1 2 -> Message Base prompt (1-1)", b"J 1 2\r")
cap("C4: 5 at base prompt -> joins base 1", b"5\r")
cap("C4: J 1 2 again -> base prompt", b"J 1 2\r")
cap("C4: blank abort at base prompt", b"\r")

cap("logoff G", b"G\r", maxwait=6)

with open(OUT, "w") as f:
    f.write("\n".join(log))
print(f"wrote {OUT}")
