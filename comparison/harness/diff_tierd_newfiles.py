#!/usr/bin/env python3
"""Slice D9 verification — scenario-window differ.

Pairs the NextExpress capture (comparison/transcripts/rust_tierd_newfiles.txt,
UTF-8 wire) against the AquaScan reference capture
(comparison/transcripts/ae_tierd_newfiles.txt, Latin-1 wire; pass 2 is
definitive, pass 1 supplies N2q and the R-12-30-26 form of N4b) and diffs each
N-command window after normalising the documented deliberate departures
(COMMAND_PARITY.md "N — new-files scan"):

  - NextScan branding: banner centre label + stretched dash run, the
    Copyright right label, the `- Configure <tool>` help row
  - UTF-8 re-encoded high-bit glyphs (handled by decoding: Latin-1 vs UTF-8 —
    comparison is at the code-point level)
  - run-time dates (mm-dd-yy) — the Rust server runs on the real clock
  - the D2f 4-column flag-marker slot on framed rows (+4 on description
    continuation indents)
  - page-2+ pager positions (uniform 29 vs the door's drifting counter) —
    checked in a content mode that tokenises the More? exchanges

Windows are scoped to the N command's own output: the Rust side is cut at its
full-menu redraw, the reference at its menu prompt. Login/J/logoff blocks are
skipped (documented seed-data / flow differences).

Usage: python3 diff_tierd_newfiles.py [RUST_TXT] [AE_TXT]
Exit status: number of scenarios with a REAL divergence (0 = parity holds).
"""
import ast
import difflib
import re
import sys

RUST = sys.argv[1] if len(sys.argv) > 1 else "comparison/transcripts/rust_tierd_newfiles.txt"
AE = sys.argv[2] if len(sys.argv) > 2 else "comparison/transcripts/ae_tierd_newfiles.txt"

MARKER = re.compile(r"^@@@@@ (.+?) @@@@@(?: \[(.*)\])?$")

RUST_MENU_ART = b"  .oO("
RUST_MENU_PROMPT = b"\x1b[0m\x1b[35mNextExpress \x1b[0m["
AE_MENU_PROMPT = b"\x1b[0m\x1b[35mNextExpress Reference \x1b[0m["

BRAND = "\x1b[36m<BRAND>\x1b[34m]<DASHES>["
DATE = re.compile(r"\d\d-\d\d-\d\d")
# The D2f slot: 4 spaces between the name field's [34m and the 1-char check
# byte, immediately before the [32m size field.
SLOT = re.compile(r"(\x1b\[34m)    (.\x1b\[32m)")
# Description continuations shift right with the slot: double reset + indent.
CONT = re.compile(r"(\x1b\[0m\x1b\[0m)    ( *\S)")
# One pager exchange: More? prompt, harness answer marker, CR + blank + CR.
PAGER = re.compile(
    r"\x1b\[0;36mMore\?[^<]*?uit:\x1b\[0m <<<harness answers uit:: (.)>>>\r +\r"
)


def parse(path, encoding):
    """Parse a harness transcript into scenario blocks.

    Returns a list of dicts: label, key (label up to the colon), status,
    and the scenario's wire bytes decoded with `encoding` (None when the
    block has no REPR section, e.g. LOGOFF rounds)."""
    lines = open(path, encoding="utf-8").read().split("\n")
    blocks = []
    starts = [i for i, ln in enumerate(lines) if MARKER.match(ln)]
    starts.append(len(lines))
    for a, b in zip(starts, starts[1:]):
        m = MARKER.match(lines[a])
        label, status = m.group(1), m.group(2)
        body = lines[a + 1 : b]
        text = None
        if "----- REPR -----" in body:
            j = body.index("----- REPR -----")
            raw = "\n".join(body[j + 1 :]).strip()
            if raw:
                text = ast.literal_eval(raw).decode(encoding)
        key = label.split(":")[0].strip()
        blocks.append({"label": label, "key": key, "status": status, "text": text})
    return blocks


def window(text, cuts):
    """Scope a scenario to the N command's own output: cut at the first
    occurrence of any cut marker (menu redraw / menu prompt)."""
    pos = len(text)
    for c in cuts:
        i = text.find(c)
        if i != -1:
            pos = min(pos, i)
    return text[:pos]


def norm_common(t):
    return DATE.sub("MM-DD-YY", t)


def norm_rust(t):
    t = re.sub(r"\x1b\[36mNextScan \x1b\[34m\]-+\[", BRAND, t)
    t = t.replace("Copyright © 2026 NextScan ", "<COPYRIGHT> ")
    t = t.replace("- Configure NextScan", "- Configure <TOOL>")
    t = SLOT.sub(r"\1\2", t)
    t = CONT.sub(r"\1\2", t)
    return norm_common(t)


def norm_ae(t):
    t = re.sub(
        r"\x1b\[36mAquaScan v1\.0 by Aquarius/Outlaws \x1b\[34m\]-+\[", BRAND, t
    )
    t = t.replace("Copyright © 1994 Aquarius ", "<COPYRIGHT> ")
    t = t.replace("- Configure AquaScan", "- Configure <TOOL>")
    return norm_common(t)


def content_mode(t):
    """Tokenise pager exchanges so page-boundary drift (documented COSMETIC)
    is factored out; returns (text-without-pager-tokens, answer-sequence)."""
    answers = PAGER.findall(t)
    return PAGER.sub("", t), answers


def show_diff(a, b, la, lb):
    out = []
    aa = [repr(x) for x in a.split("\r\n")]
    bb = [repr(x) for x in b.split("\r\n")]
    for ln in difflib.unified_diff(bb, aa, fromfile=lb, tofile=la, lineterm=""):
        out.append(ln)
    return "\n".join(out)


def main():
    rust = parse(RUST, "utf-8")
    ae = parse(AE, "latin-1")

    # Split the reference into its two passes (pass 2 starts at the second
    # CONNECT marker); pass 2 is definitive.
    connects = [i for i, blk in enumerate(ae) if blk["key"] == "CONNECT"]
    ae1 = ae[: connects[1]]
    ae2 = ae[connects[1] :]

    def find(blks, key, must=None):
        for blk in blks:
            if blk["key"] == key and (must is None or must in blk["label"]):
                return blk
        return None

    # rust key -> (reference block, note). N9 appears twice on each side —
    # the J hop is skipped (seed-data conf layout), the bare-N run compared.
    pairs = []
    for rb in rust:
        k, lbl = rb["key"], rb["label"]
        if k in ("CONNECT", "graphics -> Y", "name -> sysop",
                 "password -> POST-LOGIN", "LOGOFF round", "restore landing conf"):
            continue
        if k == "N9" and "J 2" in lbl:
            continue
        if k == "NW":
            pairs.append((rb, None, "rust-only (documented: N W unported)"))
        elif k == "N2q":
            pairs.append((rb, find(ae1, "N2q"), "pass 1; answer queues differ (Y Y Q vs Y Q)"))
        elif k == "N4c":
            pairs.append((rb, find(ae1, "N4b"), "pass 1 N4b (R 12-30-26)"))
        elif k == "N9":
            pairs.append((rb, find(ae2, "N9", must="bare N"), "pass 2"))
        else:
            pairs.append((rb, find(ae2, k), "pass 2"))

    real = 0
    for rb, ab, note in pairs:
        label = rb["label"]
        rw = window(rb["text"], [RUST_MENU_ART.decode(), RUST_MENU_PROMPT.decode()])
        if ab is None:
            ok = ("Argument error! Type 'n ?' for help." in rw
                  and "Copyright © 2026 NextScan" in rw)
            print(f"{'SELF-CHECK PASS' if ok else 'SELF-CHECK FAIL':<18} {label}  [{note}]")
            real += 0 if ok else 1
            continue
        aw = window(ab["text"], [AE_MENU_PROMPT.decode("latin-1")])
        rn, an = norm_rust(rw), norm_ae(aw)
        if rn == an:
            print(f"{'MATCH':<18} {label}  [{note}]")
            continue
        rc, ranswers = content_mode(rn)
        ac, aanswers = content_mode(an)
        if rc == ac:
            print(f"{'MATCH (content)':<18} {label}  [pager positions differ: "
                  f"rust {len(ranswers)} exchange(s) {ranswers}, ref {len(aanswers)} {aanswers}]  [{note}]")
            continue
        real += 1
        print(f"{'DIVERGENCE':<18} {label}  [{note}]")
        print(show_diff(rc, ac, "rust(normalised)", "reference(normalised)"))
        print()

    print(f"\n{real} scenario(s) with unexplained divergence")
    sys.exit(min(real, 125))


if __name__ == "__main__":
    main()
