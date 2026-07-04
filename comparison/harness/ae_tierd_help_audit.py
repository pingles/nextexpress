#!/usr/bin/env python3
"""Tier D capture — help-surface audit probes.

What does the door actually do with the advertised-but-unverified forms?

  W1   `F W` (Configure AquaScan — NextExpress keeps this unported)
  W2   `N W` (same, via the N icon)
  PK   `K` at a More? prompt in `F A` (help: Skip dir)
  PL   `L` at a More? prompt in `F 1` (help: Reload dir)
  PN   `N` at a More? prompt in `F 1` (help: (N),(Q) Quit)
  PO   `O` at a More? prompt in `F 1` (help: Who are online?)
  PC   `C` at a More? prompt in `F 1` (More? advertises (C)lear)
  PCC  Ctrl-C at a More? prompt in `F 1` (help: Quit at any time)

Deliberately NOT probed: `D` (quit-and-download — protocol/transfer
state hazard on the node), `X` (mark fake — mutates board data),
`V`/`Z`/`A` (open sub-flows; owed to their owning slices).

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

MORE = b"uit:"
FLAG = b"to flag:"
DIRS = b"=None ?"
NSCONF = b"sure "
DATE = b" ?\x1b[0m "

DEFAULTS = {MORE: b"Y", FLAG: b"\r", DIRS: b"\r", NSCONF: b"n", DATE: b"\r"}


def scenario(bbs, label, send, more=(), flag=(), dirs=(), nsconf=(), date=(),
             maxrounds=80, maxwait=45):
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
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=25)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_help_audit.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # ---- the unported W forms ----
        scenario(bbs, "W1: F W (Configure AquaScan)", b"F W\r")
        scenario(bbs, "W2: N W (Configure via the N icon)", b"N W\r")

        # ---- pager verbs at a More? prompt ----
        # K: multi-dir scan so a skip has somewhere to go.
        scenario(bbs, "PK: F A, K at first More? (Skip dir)",
                 b"F A\r", more=[b"K", b"Q"])
        scenario(bbs, "PL: F 1, L at first More? (Reload dir)",
                 b"F 1\r", more=[b"L", b"Q"])
        scenario(bbs, "PN: F 1, N at first More? (advertised Quit alias)",
                 b"F 1\r", more=[b"N"])
        scenario(bbs, "PO: F 1, O at first More? (Who are online?)",
                 b"F 1\r", more=[b"O", b"Q"])
        scenario(bbs, "PC: F 1, C at first More? (Clear)",
                 b"F 1\r", more=[b"C", b"Q"])
        scenario(bbs, "PCC: F 1, Ctrl-C at first More? (quit any time)",
                 b"F 1\r", more=[b"\x03"])
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
