#!/usr/bin/env python3
"""Capture a full NextExpress (Rust) command battery into a segmented transcript.

Rust responds instantly, so idle-based capture is reliable here.
"""
import sys
sys.path.insert(0, "/tmp")
from bbsdrive import BBS, strip_iac, render

HOST, PORT = "127.0.0.1", int(sys.argv[1]) if len(sys.argv) > 1 else 2323
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/cmp/rust_sysop.txt"

bbs = BBS(HOST, PORT, idle=0.5, maxwait=6)
log = []


def cap(label, send=None, idle=0.5, maxwait=6, note=""):
    if send is not None:
        bbs.send(send)
    data = bbs.read_idle(idle=idle, maxwait=maxwait)
    clean, _ = strip_iac(data)
    log.append(f"\n@@@@@ {label} @@@@@")
    if send is not None:
        log.append(f">>> SENT {send!r}")
    if note:
        log.append(f"(note: {note})")
    log.append("----- RENDER -----")
    log.append(render(clean))
    log.append("----- REPR -----")
    log.append(repr(clean))
    return clean


# --- login ---
cap("CONNECT / banner", None, maxwait=6)
cap("LOGIN name", b"sysop\r")
cap("LOGIN password -> menu", b"sysop\r", maxwait=6)

# --- session toggles & info ---
cap("? (redisplay menu, expert off)", b"?\r")
cap("VER", b"VER\r")
cap("T (time)", b"T\r")
cap("S (stats)", b"S\r")
cap("Q on", b"Q\r")
cap("Q off", b"Q\r")
cap("X on", b"X\r")
cap("? (redisplay menu, expert ON)", b"?\r")
cap("X off", b"X\r")
cap("H (help)", b"H\r")
cap("^ test (topic help)", b"^ test\r")
cap("^ nonexistent (topic miss)", b"^ zzznope\r")

# --- conferences ---
cap("J (no arg)", b"J\r")
cap("J 2 (join Programming)", b"J 2\r")
cap("MS in conf 2 (empty)", b"MS\r")
cap("J 1 (back to Main)", b"J 1\r")
cap("J 99 (no access)", b"J 99\r")
cap("J abc (invalid)", b"J abc\r")

# --- reading mail (R sub-prompt) ---
cap("R (no arg)", b"R\r")
cap("R 99 (not found)", b"R 99\r")
cap("R 4 (deleted)", b"R 4\r")
# Read msg 1 -> enters sub-prompt
cap("R 1 (read -> sub-prompt)", b"R 1\r")
cap("  sub: ? (short help)", b"?\r")
cap("  sub: ?? (long help)", b"??\r")
cap("  sub: A (again)", b"A\r")
cap("  sub: L (list)", b"L\r", maxwait=6)
cap("  sub: L start prompt CR", b"\r", maxwait=6)
cap("  sub: CR (advance to next)", b"\r")
cap("  sub: Q (quit to menu)", b"Q\r")
cap("R 2 (public msg) -> sub-prompt", b"R 2\r")
cap("  sub: Q", b"Q\r")

# --- MS scan all ---
cap("MS (scan all, conf 1)", b"MS\r", maxwait=6)

# --- entering mail ---
cap("E (enter msg) To prompt", b"E\r")
cap("  E To: sysop", b"sysop\r")
cap("  E Subject", b"cmp test subject\r")
cap("  E Private?", b"\r")
cap("  E body line1", b"comparison body line one\r")
cap("  E body end '.'", b".\r")

cap("C (comment to sysop) Subject", b"C\r")
cap("  C Subject", b"cmp comment subj\r")
cap("  C body", b"a comment body\r")
cap("  C body end '.'", b".\r")

# --- retired / unknown ---
cap("RP 1 (retired)", b"RP 1\r")
cap("FW 1 (retired)", b"FW 1\r")
cap("K 1 (retired)", b"K 1\r")
cap("MV 1 (retired)", b"MV 1\r")
cap("EH 1 (retired)", b"EH 1\r")
cap("N (unimplemented)", b"N\r")
cap("FOObar (unknown)", b"FOObar\r")

# --- logoff ---
cap("G (logoff)", b"G\r", maxwait=6)

bbs.close()
with open(OUT, "w") as f:
    f.write("\n".join(log))
print(f"wrote {OUT} ({len(log)} lines)")
