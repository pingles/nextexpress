#!/usr/bin/env python3
"""Full AmiExpress command battery -> segmented transcript.

Mirrors the Rust battery (rust_capture.py) on the commands NextExpress
implements, plus a few AmiExpress-only commands. Robust against the emulator's
hazards: pattern/idle reads with pause-draining, incremental save, and a
guaranteed clean `G Y` logoff in finally so the node always frees.
"""
import sys
sys.path.insert(0, "/tmp")
from ae_session import connect_until_node
from bbsdrive import render

OUT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/cmp/ae_sysop.txt"
log = []


def flush():
    with open(OUT, "w") as f:
        f.write("\n".join(log))


def record(label, sent, data, status=""):
    log.append(f"\n@@@@@ {label} @@@@@ {status}")
    if sent is not None:
        log.append(f">>> SENT {sent!r}")
    log.append("----- RENDER -----")
    log.append(render(data))
    log.append("----- REPR -----")
    log.append(repr(data))
    flush()


def drain_to_menu(bbs, maxrounds=16, idle=2.0, roundwait=18):
    """Read until the menu prompt returns, auto-answering (Pause) prompts with
    space and nudging stuck sub-prompts with Q. Returns accumulated bytes."""
    acc = bytearray()
    for _ in range(maxrounds):
        clean = bbs.read_idle(idle=idle, maxwait=roundwait)
        acc += clean
        if b"mins. left): " in bytes(acc)[-220:]:
            break
        low = clean.lower()
        if b"resume" in low or b"(pause)" in low:
            bbs.send(b" "); continue
        if clean == b"":
            bbs.send(b"\r"); continue
        stripped = low.rstrip()
        if stripped.endswith(b">:") or b"options:" in low:
            bbs.send(b"Q\r"); continue
    return bytes(acc)


def cmd(bbs, label, send, pre=None):
    """Send a command; optionally feed `pre` (list of (wait_idle, send) follow-up
    inputs for interactive prompts) before draining back to the menu."""
    bbs.send(send)
    acc = bytearray()
    if pre:
        for idle, payload in pre:
            acc += bbs.read_idle(idle=idle, maxwait=20)
            if payload is not None:
                bbs.send(payload)
    acc += drain_to_menu(bbs)
    record(label, send, bytes(acc))


def main():
    bbs, banner = connect_until_node("127.0.0.1", 6023, retries=60, log=log)
    flush()
    try:
        # --- login ---
        bbs.send(b"A\r")
        c, _ = bbs.read_until(b"Name:", maxwait=70)
        record("login: graphics A", b"A\r", c)
        bbs.send(b"sysop\r")
        c = bbs.read_idle(idle=3.0, maxwait=40)
        record("login: name", b"sysop\r", c)
        if b"already logged" in c:
            log.append("\n!!! ABORT: sysop already logged in"); flush(); bbs.close(); return
        bbs.send(b"sysop\r")
        c = drain_to_menu(bbs, maxrounds=20)
        record("login: password -> POST-LOGIN + MENU", b"sysop\r", c)

        # --- expert ON for terse menus (faster, less pagination) ---
        cmd(bbs, "X (expert toggle #1)", b"X\r")
        # --- info / toggles ---
        cmd(bbs, "VER", b"VER\r")
        cmd(bbs, "T (time)", b"T\r")
        cmd(bbs, "S (user stats)", b"S\r")
        cmd(bbs, "Q (quiet #1)", b"Q\r")
        cmd(bbs, "Q (quiet #2)", b"Q\r")
        cmd(bbs, "M (ansi #1)", b"M\r")
        cmd(bbs, "M (ansi #2)", b"M\r")
        cmd(bbs, "? (redisplay menu, expert on)", b"?\r")
        cmd(bbs, "H (help?)", b"H\r")
        # --- conferences ---
        cmd(bbs, "J 1 (join conf 1)", b"J 1\r")
        cmd(bbs, "J 2 (join conf 2 Amiga)", b"J 2\r")
        cmd(bbs, "J 99 (invalid/no access)", b"J 99\r")
        cmd(bbs, "J (no arg)", b"J\r")
        # --- mail: safe captures (abort before the editor) ---
        cmd(bbs, "E ZZZNOBODY (unknown recipient)", b"E ZZZNOBODY\r")
        cmd(bbs, "E sysop -> Subject (blank=abort)", b"E sysop\r",
            pre=[(2.5, b"\r")])
        cmd(bbs, "C (comment) -> Subject (blank=abort)", b"C\r",
            pre=[(2.5, b"\r")])
        # --- read / scan ---
        cmd(bbs, "R (read messages)", b"R\r")
        cmd(bbs, "MS (mail scan)", b"MS\r")
        cmd(bbs, "N (new files scan)", b"N\r")
        # --- unknown ---
        cmd(bbs, "FOOBAR (unknown command)", b"FOOBAR\r")
    finally:
        # Escape any lingering prompt, then force a clean logoff.
        for esc in (b" ", b"\r", b"Q\r", b"\r"):
            try:
                bbs.send(esc); bbs.read_idle(idle=1.0, maxwait=6)
            except OSError:
                break
        try:
            bbs.send(b"G Y\r")
            for _ in range(6):
                cc = bbs.read_idle(idle=1.5, maxwait=12)
                log.append("\n@@ logoff @@"); log.append(render(cc))
                if cc == b"" or b"carrier" in cc.lower() or b"click" in cc.lower():
                    break
        except OSError:
            pass
        bbs.close()
        flush()
    print("AE BATTERY DONE")


if __name__ == "__main__":
    main()
