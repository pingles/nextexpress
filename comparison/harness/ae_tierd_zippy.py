#!/usr/bin/env python3
"""Tier D capture — `Z` (zippy text search), the genuine internal command.

`Z` is NOT in the AquaScan door icon set (CS, F, FR, N, NSU, SCAN, SENT),
so it runs `internalCommandZ` (express.e:26123) straight, even with the
AquaScan icons installed — no icon-disabling needed.

Flow: `Z` -> `Enter string to search for: ` -> the search token ->
`internalCommandZ` calls `getDirSpan('')` (express.e:26862) which PROMPTS
`Directories: (1-N), ... =none? ` -> the dir answer -> `zippy()` dumps the
raw DIR rows of every file whose block contains the (upper-cased) token,
under `Scanning directory N` headers.

Scenarios target the seeded Conf02/Dir1 (28 entries) so the matches are
small and predictable (no pager). Every session ends with a clean `G Y`
logoff (FS-UAE node-spin hazard).
"""
import sys
import os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, emit, read_until_any,
    HOST, PORT, MENU_SENTINEL, PAUSE_SENTINEL, connect_until_node,
)
from bbsdrive import render  # noqa: E402

SEARCH_PROMPT = b"search for: "
DIRSPAN_PROMPT = b"=none? "


def zippy(bbs, label, first, search=None, dirspan=None):
    """Drive one Z scenario, capturing every byte to the next menu prompt.

    `first` is the initial line (e.g. b"Z\\r" or b"Z ZMODEM\\r"). If the
    search-string prompt appears, `search` is sent (b"\\r" to abort). If
    the getDirSpan prompt appears, `dirspan` is sent (b"1\\r", b"A\\r", or
    b"\\r" for =none). Pause gates are answered with a space.
    """
    bbs.send(first)
    collected = b""
    status = "TIMEOUT"
    sent_search = False
    sent_dir = False
    for _ in range(12):
        clean, hit = read_until_any(
            bbs,
            [SEARCH_PROMPT, DIRSPAN_PROMPT, PAUSE_SENTINEL, MENU_SENTINEL],
            maxwait=40,
        )
        collected += clean
        if hit == MENU_SENTINEL:
            status = "MENU"
            break
        if hit == SEARCH_PROMPT and not sent_search:
            collected += b"<<<harness search>>>" + (search or b"")
            bbs.send(search if search is not None else b"\r")
            sent_search = True
            continue
        if hit == DIRSPAN_PROMPT and not sent_dir:
            collected += b"<<<harness dir>>>" + (dirspan or b"")
            bbs.send(dirspan if dirspan is not None else b"\r")
            sent_dir = True
            continue
        if hit == PAUSE_SENTINEL:
            collected += b"<<<harness SPACE>>>"
            bbs.send(b" ")
            continue
        # Unmatched read (e.g. prompt already consumed) — keep looping.
        break
    emit(label, first, collected, status)
    return collected


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_zippy.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Tier D fixture)", b"J 2\r")

        # Z1: prompt path, multi-line block match (STARVIEW has a
        # continuation line) in dir 1 — pins search prompt + getDirSpan
        # prompt + whole-block dump.
        zippy(bbs, "Z1: Z -> STARVIEW -> dir 1 (multi-line block)",
              b"Z\r", search=b"STARVIEW\r", dirspan=b"1\r")

        # Z2: inline string path (skips the search prompt), single-line
        # match (ZMODEM matches XPRZMODM's description only).
        zippy(bbs, "Z2: Z ZMODEM (inline) -> dir 1 (single line)",
              b"Z ZMODEM\r", dirspan=b"1\r")

        # Z3: empty search string at the prompt — StrLen(ss)=0 returns to
        # the menu (express.e:26155-26156).
        zippy(bbs, "Z3: Z -> blank search (abort)", b"Z\r", search=b"\r")

        # Z4: no-match token in dir 1 — headers/blanks but no file rows.
        zippy(bbs, "Z4: Z ZqzNoMatch (inline) -> dir 1 (no match)",
              b"Z ZqzNoMatch\r", dirspan=b"1\r")

        # Z5: blank at the getDirSpan prompt = (Enter)=none -> FAILURE
        # path returns to menu (express.e:26871-26873).
        zippy(bbs, "Z5: Z -> ANSI -> blank dir (=none abort)",
              b"Z\r", search=b"ANSI\r", dirspan=b"\r")

        # Z6 (forward-looking, D7): inline string + (A)ll dirs span —
        # multi-dir walk. PROTRACKER matches 3 files in dir 1.
        zippy(bbs, "Z6: Z PROTRACKER (inline) -> A (all dirs)",
              b"Z PROTRACKER\r", dirspan=b"A\r")

        # Z7: case-insensitivity — lower-case token must match
        # (UpperStr on both sides, express.e:26160/27542).
        zippy(bbs, "Z7: Z starview (lowercase, inline) -> dir 1",
              b"Z starview\r", dirspan=b"1\r")
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
