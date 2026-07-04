#!/usr/bin/env python3
"""Slice D9 verification — replay the AquaScan `N` (new-files scan) battery
against a running NextExpress binary and capture the wire bytes for
side-by-side comparison with the reference capture
(comparison/transcripts/ae_tierd_newfiles.txt, pass 2 = definitive).

Mirrors comparison/harness/ae_tierd_newfiles.py scenario-for-scenario (same
labels, same sub-prompt answer queues, same emit format) so a scenario-window
differ can pair the two transcripts. The Rust login flow differs from the
FS-UAE reference: no `A/r/n` graphics line — the server asks
`ANSI Graphics (Y/n)? `, then `Enter your Name: `, then `PassWord: `.

Server: cargo run --manifest-path rust/Cargo.toml -- nextexpress.toml
        (127.0.0.1:2323, seeded sysop/sysop, demo corpus in conference 1,
        conference 2 = Programming, empty Dir1 — the N9 stand-in).

Usage: python3 rust_tierd_newfiles.py [PORT] [OUT]
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bbsdrive import BBS, strip_iac, render  # noqa: E402

HOST = "127.0.0.1"
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 2323
OUT = sys.argv[2] if len(sys.argv) > 2 else "/tmp/rust_tierd_newfiles.txt"

MENU_SENTINEL = b"mins. left): "
PAUSE_SENTINEL = b"Space To Resume"

# Sub-prompt sentinels — identical to ae_tierd_newfiles.py so the harness
# markers embedded in the collected bytes render the same on both sides.
MORE = b"uit:"            # More? (Y/n/ns), ... (Q)uit:
FLAG = b"to flag:"        # File name(s) to flag:
DIRS = b"=None ?"         # Directories: ... (Enter)=None ?
NSCONF = b"sure "         # Non-stop scrolling! Are you sure (Y/n)?
DATE = b" ?\x1b[0m "      # Date: ... (Enter)=MM-DD-YY ?<ESC>[0m<SP>

DEFAULTS = {MORE: b"Y", FLAG: b"\r", DIRS: b"\r", NSCONF: b"n", DATE: b"\r"}

LOG = []


def emit(label, sent, clean, status):
    LOG.append(f"\n@@@@@ {label} @@@@@ [{status}]")
    if sent is not None:
        LOG.append(f">>> SENT {sent!r}")
    LOG.append("----- RENDER -----")
    LOG.append(render(clean))
    LOG.append("----- REPR -----")
    LOG.append(repr(clean))


def read_until_any(bbs, patterns, maxwait=20):
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


def to_menu(bbs, label, send=None, maxwait=20):
    if send is not None:
        bbs.send(send)
    collected = b""
    status = "TIMEOUT"
    for _ in range(8):
        clean, hit = read_until_any(bbs, [MENU_SENTINEL, PAUSE_SENTINEL],
                                    maxwait=maxwait)
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


def to_pattern(bbs, label, send, pattern, maxwait=20):
    if send is not None:
        bbs.send(send)
    clean, hit = read_until_any(bbs, [pattern, PAUSE_SENTINEL, MENU_SENTINEL],
                                maxwait=maxwait)
    status = "MATCHED" if hit == pattern else f"GOT {hit!r}"
    emit(label, send, clean, status)
    return clean, hit


def scenario(bbs, label, send, more=(), flag=(), dirs=(), nsconf=(), date=(),
             maxrounds=80, maxwait=20):
    """Send `send`, then answer every known sub-prompt from per-kind queues
    (safe defaults when a queue runs dry) until the menu prompt returns.
    Recovers to the menu (Q, CR) on surprise so scenarios cannot cascade."""
    queues = {MORE: list(more), FLAG: list(flag), DIRS: list(dirs),
              NSCONF: list(nsconf), DATE: list(date)}
    bbs.send(send)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG, DIRS, NSCONF, MORE, DATE,
                  PAUSE_SENTINEL],
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
        for esc in (b"Q", b"\r", b"q\r"):
            bbs.send(esc)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=10)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    bbs = BBS(HOST, PORT, idle=0.5, maxwait=10)
    try:
        # ---- login (NextExpress flow) ----
        clean, _ = read_until_any(bbs, [b"Graphics (Y/n)? "], maxwait=15)
        emit("CONNECT", None, clean, "BANNER")
        to_pattern(bbs, "graphics -> Y", b"Y\r", b"Name: ", maxwait=15)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assWord", maxwait=15)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=30)
        # The demo corpus lives in conference 1 (Main) — the landing
        # conference; no J needed (the reference held it in conf 2).

        # ---- the advertised help screen (grammar reference) ----
        scenario(bbs, "N6: N ? (options/help screen)", b"N ?\r")

        # ---- bare-N prompt paths ----
        scenario(bbs, "N1a: bare N -> Enter date -> Enter dirs (None abort)",
                 b"N\r", date=[b"\r"], dirs=[b"\r"])
        scenario(bbs, "N1b: bare N -> Enter date -> dir 2",
                 b"N\r", date=[b"\r"], dirs=[b"2\r"])
        scenario(bbs, "N1c: bare N -> Enter date -> A (all dirs)",
                 b"N\r", date=[b"\r"], dirs=[b"A\r"])

        # N2: broad date + all dirs, page to the natural end (default Y).
        scenario(bbs, "N2: bare N -> 01-01-26 -> A, page to end",
                 b"N\r", date=[b"01-01-26\r"], dirs=[b"A\r"])
        # N2q: same scan, Y twice then Q mid-list (parent-task battery).
        scenario(bbs, "N2q: bare N -> 01-01-26 -> A, Y Y then Q",
                 b"N\r", date=[b"01-01-26\r"], dirs=[b"A\r"],
                 more=[b"Y", b"Y", b"Q"])

        scenario(bbs, "N3: bare N -> -30 -> dir 1",
                 b"N\r", date=[b"-30\r"], dirs=[b"1\r"], more=[b"Y", b"Q"])

        scenario(bbs, "N4: bare N -> R -> dir 2 (reverse)",
                 b"N\r", date=[b"R\r"], dirs=[b"2\r"])
        scenario(bbs, "N4b: bare N -> R 01-01-26 -> dir 2",
                 b"N\r", date=[b"R 01-01-26\r"], dirs=[b"2\r"])
        # The parent task's exact form (pass-1 pairing): R + future date.
        scenario(bbs, "N4c: bare N -> R 12-30-26 -> dir 2",
                 b"N\r", date=[b"R 12-30-26\r"], dirs=[b"2\r"])

        scenario(bbs, "N5: bare N -> FOO (junk date)",
                 b"N\r", date=[b"FOO\r"])

        scenario(bbs, "N8: bare N -> 12-30-26 -> A (matches nothing)",
                 b"N\r", date=[b"12-30-26\r"], dirs=[b"A\r"])
        scenario(bbs, "N8b: bare N -> Enter date -> dir 9 (out of range)",
                 b"N\r", date=[b"\r"], dirs=[b"9\r"])

        # ---- inline argument forms (no prompts) ----
        scenario(bbs, "N7a: N 01-01-26 (inline date, default dir=upload)",
                 b"N 01-01-26\r")
        scenario(bbs, "N7b: N -30 (inline days)", b"N -30\r")
        scenario(bbs, "N7c: N 01-01-26 1 (inline date + dir, quit mid-list)",
                 b"N 01-01-26 1\r", more=[b"Y", b"Q"])
        scenario(bbs, "N7q: N 01-01-26 2 Q (quick scan: first line only)",
                 b"N 01-01-26 2 Q\r")
        scenario(bbs, "N7ns: N 01-01-26 2 NS (non-stop scroll)",
                 b"N 01-01-26 2 NS\r")
        scenario(bbs, "N7n: N !2 (the 2 newest files)", b"N !2\r")
        scenario(bbs, "N7t: N T (today)", b"N T\r")
        scenario(bbs, "N7y: N Y (yesterday)", b"N Y\r")
        scenario(bbs, "N7s: N S (explicit since-last-call)", b"N S\r")
        scenario(bbs, "N7d: N 2 (bare dir token)", b"N 2\r")
        scenario(bbs, "N7r: N R 2 (inline reverse, dir 2)", b"N R 2\r")
        scenario(bbs, "N7e: N R -1 (invalid combo -> Argument error!)",
                 b"N R -1\r")
        # Help-advertised but deliberately unported (COMMAND_PARITY row).
        scenario(bbs, "NW: N W (unported -> Argument error)", b"N W\r")

        # ---- empty-conference shape (conf 2 = Programming, Dir1 empty) ----
        to_menu(bbs, "N9: J 2 (empty Dir1 conf)", b"J 2\r")
        scenario(bbs, "N9: bare N -> Enter date -> dir 1 (empty dir)",
                 b"N\r", date=[b"\r"], dirs=[b"1\r"])
        to_menu(bbs, "restore landing conf: J 1", b"J 1\r")
    finally:
        try:
            bbs.send(b"G\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.0, maxwait=8)
                LOG.append("\n@@@@@ LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(OUT, "w") as f:
            f.write("\n".join(LOG))
        print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
