#!/usr/bin/env python3
"""Tier D6 capture — the genuine internal `A` (alterFlags).

`A` is NOT door-shadowed, so typing `A` at the menu runs internalCommandA
(express.e:24601) -> alterFlags(params) (express.e:12648):

    \\b\\n  showFlags()  [ backloop: 'Filename(s) to flag: (F)rom, (C)lear,
    (Enter)=none? ' lineInput ]  ... \\b\\n

showFlags (express.e:12486): `No file flags\\b\\n` when empty, else
showFlaggedFiles(-1) (space-joined, UpperStr'd names) + `\\b\\n`.

addFlagToList (express.e:12555) does NOT check the file exists on disk —
it just UpperStr's and adds the name — so we flag names directly at the
internal prompt (lowercase input proves the UpperStr fold) without the
AquaScan F door. This captures the D6a listing surface (empty / 1 / 2
names) and the D6b prompt surface (main prompt, add path, `C`lear
sub-prompt, `*`=All) in one genuine-internal session.

We leave the persisted flag set EMPTY at the end (C *) and log off with
`G Y` (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, read_until_any, emit,
    HOST, PORT, MENU_SENTINEL,
)
from ae_session import connect_until_node  # noqa: E402
from bbsdrive import render  # noqa: E402

FLAG_PROMPT = b"to flag:"     # 'Filename(s) to flag: ...? '
CLEAR_PROMPT = b"to Clear:"   # 'Filename(s) to Clear: (*)All, ...? '


def step(bbs, label, send, wait_for, maxwait=30):
    """Send `send`, read until `wait_for` (or the menu), emit, return clean."""
    bbs.send(send)
    clean, hit = read_until_any(bbs, [wait_for, MENU_SENTINEL], maxwait=maxwait)
    status = "MATCHED" if hit == wait_for else f"GOT {hit!r}"
    emit(label, send, clean, status)
    return clean, hit


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_alterflags.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        login = to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        emit("BANNER CHECK (D5-persist logon banner)", None,
             b"** Flagged File(s) Exist ** PRESENT at login"
             if b"Flagged File(s) Exist" in login
             else b"(no saved-flag banner at login)", "info")

        to_menu(bbs, "J 2 (Amiga, seeded files)", b"J 2\r")

        # --- A: show current flags + the main prompt (whatever persisted) ---
        step(bbs, "A -> listing + flag prompt", b"A\r", FLAG_PROMPT)

        # --- C -> the Clear sub-prompt; * = All -> cleared -> 'No file flags' ---
        step(bbs, "at prompt: C -> Clear sub-prompt", b"C\r", CLEAR_PROMPT)
        step(bbs, "at Clear prompt: * -> cleared, empty listing", b"*\r", FLAG_PROMPT)

        # --- flag two names inline (lowercase input -> UpperStr in the list) ---
        step(bbs, "flag 'mydemo.dms' -> 1-name listing", b"mydemo.dms\r", FLAG_PROMPT)
        step(bbs, "flag 'report.txt' -> 2-name listing", b"report.txt\r", FLAG_PROMPT)

        # --- Enter (=none) exits alterFlags back to the menu ---
        to_menu(bbs, "Enter (=none) -> back to menu", b"\r")

        # --- A again: the canonical clean 2-name listing + prompt ---
        step(bbs, "A again -> clean 2-name listing", b"A\r", FLAG_PROMPT)

        # --- cleanup: C * to leave the persisted set empty, then Enter ---
        step(bbs, "cleanup: C -> Clear sub-prompt", b"C\r", CLEAR_PROMPT)
        step(bbs, "cleanup: * -> empty", b"*\r", FLAG_PROMPT)
        to_menu(bbs, "cleanup: Enter -> menu", b"\r")
    finally:
        try:
            bbs.send(b"G Y\r")
            for _ in range(6):
                clean = bbs.read_idle(idle=1.5, maxwait=10)
                LOG.append("\n@@@@@ SAFETY LOGOFF round @@@@@")
                LOG.append(render(clean))
                if clean == b"":
                    break
        except OSError:
            pass
        bbs.close()
        with open(out_path, "w") as f:
            f.write("\n".join(LOG))
        print("wrote", out_path)


if __name__ == "__main__":
    main()
