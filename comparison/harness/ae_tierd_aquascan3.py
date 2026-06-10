#!/usr/bin/env python3
"""Tier D capture 3 — AquaScan surgical pass with every sub-prompt known.

Sub-prompts discovered in the first two passes (ae_tierd_aquascan.txt):
  More?  -> `More? (Y/n/ns), (C)lear, (F/R) Flag, (?) Help, (Q)uit:` hotkeys
  Flag   -> `File name(s) to flag:` line input
  Dirs   -> AquaScan's own `Directories: (1-2), (A)ll, (U)pload, (H)old,
            (Enter)=None ? ` line input (bare F); bad input -> `Error in
            input!` and exit to menu
  NsConf -> `Non-stop scrolling! Are you sure (Y/n)? ` hotkey confirm

Each scenario drives its prompts from per-kind answer queues with safe
defaults, and recovers to the menu (Q, CR) if it runs out of rounds, so a
surprise prompt can never cascade into the next scenario.

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

MORE = b"uit:"            # ...(Q)uit:
FLAG = b"to flag:"        # File name(s) to flag:
DIRS = b"=None ?"         # AquaScan Directories: ... (Enter)=None ?
NSCONF = b"sure "         # Non-stop scrolling! Are you sure (Y/n)?

DEFAULTS = {MORE: b"Y", FLAG: b"\r", DIRS: b"\r", NSCONF: b"n"}


def scenario(bbs, label, send, more=(), flag=(), dirs=(), nsconf=(),
             maxrounds=60, maxwait=45):
    queues = {MORE: list(more), FLAG: list(flag),
              DIRS: list(dirs), NSCONF: list(nsconf)}
    bbs.send(send)
    collected = b""
    status = "RAN OUT OF ROUNDS"
    for _ in range(maxrounds):
        clean, hit = read_until_any(
            bbs, [MENU_SENTINEL, FLAG, DIRS, NSCONF, MORE, PAUSE_SENTINEL],
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
        # Recover to the menu so the next scenario starts clean.
        for esc in (b"Q", b"\r", b"q\r"):
            bbs.send(esc)
            clean, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=20)
            collected += b"<<<recovery %s>>>" % esc + clean
            if hit == MENU_SENTINEL:
                status += " +RECOVERED"
                break
    emit(label, send, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_aquascan3.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        scenario(bbs, "S1: F ? (options/help screen)", b"F ?\r")
        scenario(bbs, "S2: bare F -> answer 2 -> stop with n",
                 b"F\r", dirs=[b"2\r"], more=[b"n"])
        scenario(bbs, "S3: bare F -> Enter (None abort)",
                 b"F\r", dirs=[b"\r"])
        scenario(bbs, "S4: F 1 -> Flag TERMV48.LHA -> Quit",
                 b"F 1\r", more=[b"F", b"Q"], flag=[b"TERMV48.LHA\r"])
        scenario(bbs, "S5: F 1 -> R (reverse-flag?) -> Quit",
                 b"F 1\r", more=[b"R", b"Q"], flag=[b"ANSIPACK.LHA\r"])
        scenario(bbs, "S6: F 1 -> C (clear) -> Quit",
                 b"F 1\r", more=[b"C", b"Q"])
        scenario(bbs, "S7: F 1 -> ns -> confirm Y (non-stop to end)",
                 b"F 1\r", more=[b"ns"], nsconf=[b"Y"])
        scenario(bbs, "S8: F A (all dirs, page through)", b"F A\r")
        scenario(bbs, "S9: F H (hold via AquaScan)", b"F H\r")
        scenario(bbs, "S10: FR 1 (reverse listing)", b"FR 1\r")
        scenario(bbs, "S11: bare FR", b"FR\r", dirs=[b"1\r"], more=[b"n"])
        scenario(bbs, "S12: N (new files via AquaScan)", b"N\r")
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
