#!/usr/bin/env python3
"""Post real messages into AmiExpress and read them back, capturing how the
legacy system renders message content (header, body, populated read sub-prompt,
message list, mail scan). Always logs off cleanly in finally."""
import sys
sys.path.insert(0, "/tmp")
from ae_session import connect_until_node
from bbsdrive import render

OUT = "/tmp/cmp/ae_content.txt"
log = []


def flush():
    open(OUT, "w").write("\n".join(log))


def rec(label, data, status=""):
    log.append(f"\n@@@@@ {label} @@@@@ {status}")
    log.append("----- RENDER -----")
    log.append(render(data))
    log.append("----- REPR -----")
    log.append(repr(data))
    flush()


def until(bbs, pat, maxwait=40):
    clean, ok = bbs.read_until(pat if isinstance(pat, bytes) else pat.encode(), maxwait=maxwait)
    return clean, ok


def drain_menu(bbs, maxrounds=14, idle=2.0, roundwait=18):
    acc = bytearray()
    for _ in range(maxrounds):
        c = bbs.read_idle(idle=idle, maxwait=roundwait)
        acc += c
        if b"mins. left): " in bytes(acc)[-220:]:
            break
        low = c.lower()
        if b"resume" in low or b"(pause)" in low:
            bbs.send(b" "); continue
        if c == b"":
            bbs.send(b"\r"); continue
    return bytes(acc)


def post(bbs, to, subject, private_char, body_lines):
    acc = bytearray()
    bbs.send(("E " + to + "\r").encode())
    c, _ = until(bbs, "Subject", 40); acc += c
    bbs.send((subject + "\r").encode())
    c, _ = until(bbs, "Private", 30); acc += c
    bbs.send(private_char)                      # Private (y/N) single keystroke
    c, _ = until(bbs, "Editor", 25); acc += c   # "FullScreen Editor (y/N)?"
    bbs.send(b"N")                              # decline -> built-in LINE editor
    c = bbs.read_idle(idle=2.5, maxwait=25); acc += c   # line editor entry
    for ln in body_lines:
        bbs.send((ln + "\r").encode())
    bbs.send(b"\r")                              # blank line -> "Msg. Options:" save menu
    c, _ = until(bbs, "Options:", 30); acc += c
    bbs.send(b"S\r")                             # S>ave
    c = drain_menu(bbs); acc += c
    rec(f"POST to={to!r} subj={subject!r} priv={private_char!r}", bytes(acc))


def main():
    bbs, banner = connect_until_node("127.0.0.1", 6023, retries=60, log=log)
    flush()
    try:
        bbs.send(b"A\r"); until(bbs, "Name:", 70)
        bbs.send(b"sysop\r"); until(bbs, "assword", 30)
        bbs.send(b"sysop\r")
        c = drain_menu(bbs, maxrounds=20)
        rec("login -> menu", c)
        bbs.send(b"X\r"); c = drain_menu(bbs); rec("X expert on", c)   # terse

        # --- create content ---
        post(bbs, "sysop", "Comparison harness: private note", b"Y",
             ["This is a PRIVATE message addressed to sysop.",
              "Posted live via the comparison harness to show",
              "how AmiExpress renders message bodies."])
        post(bbs, "ALL", "Comparison harness: public bulletin", b"N",
             ["This is a PUBLIC message addressed to ALL.",
              "Second line of the public bulletin."])

        # --- read it back (populated sub-prompt) ---
        bbs.send(b"R\r")
        c, _ = until(bbs, "Options:", 40)
        rec("R -> first message + sub-prompt", c)
        bbs.send(b"\r")                          # <CR> advance to next message
        c, _ = until(bbs, "Options:", 30)
        rec("sub: <CR> advance to next msg", c)
        bbs.send(b"A\r")                         # A>gain
        c, _ = until(bbs, "Options:", 30)
        rec("sub: A (again)", c)
        bbs.send(b"L\r")                         # L>ist
        c = bbs.read_idle(idle=2.5, maxwait=30)
        rec("sub: L (list messages)", c)
        # L may prompt for a start message; nudge with CR and drain to a prompt
        bbs.send(b"\r"); c = bbs.read_idle(idle=2.5, maxwait=25)
        rec("sub: L (after start CR)", c)
        bbs.send(b"?\r"); c = bbs.read_idle(idle=2.0, maxwait=20)
        rec("sub: ? (short help)", c)
        bbs.send(b"??\r"); c = bbs.read_idle(idle=2.0, maxwait=20)
        rec("sub: ?? (long help)", c)
        bbs.send(b"Q\r"); c = drain_menu(bbs)
        rec("sub: Q (quit to menu)", c)

        # --- mail scan with mail present ---
        bbs.send(b"MS\r")
        c = bbs.read_idle(idle=2.5, maxwait=35)
        rec("MS (mail scan, mail present)", c)
        # MS may ask 'Would you like to read it now' -> decline with N
        if b"read it now" in c.lower() or b"(y/n)" in c.lower() or c.rstrip().endswith(b"? "):
            bbs.send(b"N\r")
        c = drain_menu(bbs)
        rec("MS (after decline read-it-now)", c)
    finally:
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
        bbs.close(); flush()
    print("AE CONTENT DONE")


if __name__ == "__main__":
    main()
