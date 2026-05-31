#!/usr/bin/env python3
"""Telnet driver for comparing NextExpress (Rust) and AmiExpress (legacy).

Usage:
    python3 bbsdrive.py HOST PORT [--idle SECS] [--maxwait SECS] < script
    python3 bbsdrive.py HOST PORT --script FILE

Script directives (one per line):
    # comment                      ignored
    expect_idle [SECS]             read until the stream is idle for `idle`
                                   seconds (cap SECS, default --maxwait); emit
                                   a captured block
    send TEXT                      send TEXT with C escapes (\\r \\n \\t \\xNN
                                   \\e); does NOT auto-append CR
    sendline TEXT                  like `send` but appends \\r
    sleep SECS                     passive sleep (no read)

Output: a transcript to stdout. Each captured block is shown twice — once as a
human-readable RENDER (ANSI/control bytes made visible) and once as a raw
Python repr() so escape sequences and exact bytes are auditable. Telnet IAC
negotiation is parsed out of the captured stream (and logged separately) so it
does not corrupt the comparison.
"""
import socket
import sys
import time

IAC = 255
SB, SE = 250, 240
WILL, WONT, DO, DONT = 251, 252, 253, 254


def unescape(s: str) -> bytes:
    out = bytearray()
    i = 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            if n == "r":
                out.append(0x0D); i += 2; continue
            if n == "n":
                out.append(0x0A); i += 2; continue
            if n == "t":
                out.append(0x09); i += 2; continue
            if n == "e":
                out.append(0x1B); i += 2; continue
            if n == "0":
                out.append(0x00); i += 2; continue
            if n == "\\":
                out.append(0x5C); i += 2; continue
            if n == "x" and i + 3 < len(s):
                out.append(int(s[i + 2:i + 4], 16)); i += 4; continue
        out.append(ord(c)); i += 1
    return bytes(out)


def strip_iac(data: bytes):
    """Return (clean_bytes, iac_events). Parses telnet commands out of `data`."""
    clean = bytearray()
    events = []
    i = 0
    while i < len(data):
        b = data[i]
        if b == IAC:
            if i + 1 >= len(data):
                break
            cmd = data[i + 1]
            if cmd == IAC:
                clean.append(IAC); i += 2; continue
            if cmd in (WILL, WONT, DO, DONT):
                if i + 2 < len(data):
                    opt = data[i + 2]
                    name = {WILL: "WILL", WONT: "WONT", DO: "DO", DONT: "DONT"}[cmd]
                    events.append(f"IAC {name} {opt}")
                    i += 3; continue
                i += 2; continue
            if cmd == SB:
                j = i + 2
                while j + 1 < len(data) and not (data[j] == IAC and data[j + 1] == SE):
                    j += 1
                events.append("IAC SB ... SE")
                i = j + 2; continue
            events.append(f"IAC {cmd}")
            i += 2; continue
        clean.append(b); i += 1
    return bytes(clean), events


def render(data: bytes) -> str:
    """Make control/ANSI bytes visible for human reading."""
    out = []
    i = 0
    while i < len(data):
        b = data[i]
        if b == 0x1B:  # ESC — show CSI sequences compactly
            j = i + 1
            if j < len(data) and data[j] == 0x5B:  # [
                k = j + 1
                while k < len(data) and not (0x40 <= data[k] <= 0x7E):
                    k += 1
                seq = data[i:k + 1].decode("latin1")
                out.append("⟨ESC" + seq[1:] + "⟩")
                i = k + 1; continue
            out.append("⟨ESC⟩"); i += 1; continue
        if b == 0x0D:
            out.append("\\r"); i += 1; continue
        if b == 0x0A:
            out.append("\\n\n"); i += 1; continue
        if b == 0x08:
            out.append("⟨BS⟩"); i += 1; continue
        if b == 0x07:
            out.append("⟨BEL⟩"); i += 1; continue
        if b == 0x00:
            out.append("⟨NUL⟩"); i += 1; continue
        if 32 <= b < 127:
            out.append(chr(b)); i += 1; continue
        out.append(f"⟨{b:02x}⟩"); i += 1
    return "".join(out)


class BBS:
    def __init__(self, host, port, idle=0.8, maxwait=12.0, connect_timeout=15):
        self.sock = socket.create_connection((host, port), timeout=connect_timeout)
        self.idle = idle
        self.maxwait = maxwait
        self.all_raw = bytearray()

    def read_idle(self, idle=None, maxwait=None) -> bytes:
        idle = self.idle if idle is None else idle
        maxwait = self.maxwait if maxwait is None else maxwait
        chunks = bytearray()
        start = time.time()
        last = time.time()
        self.sock.settimeout(0.25)
        while True:
            if time.time() - start > maxwait:
                break
            try:
                data = self.sock.recv(4096)
                if data == b"":
                    break
                chunks += data
                self.all_raw += data
                last = time.time()
            except socket.timeout:
                if chunks and (time.time() - last) >= idle:
                    break
                if not chunks and (time.time() - start) >= maxwait:
                    break
        return bytes(chunks)

    def read_until(self, pattern: bytes, maxwait=None):
        """Read until `pattern` (matched against IAC-stripped output) appears,
        or maxwait elapses. Returns (clean_bytes, matched_bool)."""
        maxwait = self.maxwait if maxwait is None else maxwait
        chunks = bytearray()
        clean = b""
        start = time.time()
        self.sock.settimeout(0.25)
        while time.time() - start < maxwait:
            try:
                data = self.sock.recv(4096)
                if data == b"":
                    break
                chunks += data
                self.all_raw += data
                clean, _ = strip_iac(bytes(chunks))
                if pattern in clean:
                    return clean, True
            except socket.timeout:
                continue
        return clean, (pattern in clean)

    def send(self, data: bytes):
        self.sock.sendall(data)

    def close(self):
        try:
            self.sock.close()
        except OSError:
            pass


def run_script(bbs: BBS, lines):
    n = 0
    for raw in lines:
        line = raw.rstrip("\n")
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split(" ", 1)
        cmd = parts[0]
        arg = parts[1] if len(parts) > 1 else ""
        if cmd == "expect_idle":
            mw = float(arg) if arg.strip() else bbs.maxwait
            data = bbs.read_idle(maxwait=mw)
            clean, events = strip_iac(data)
            n += 1
            print(f"\n===== CAPTURE #{n} ({len(data)} bytes raw, {len(clean)} clean) =====")
            if events:
                print("  [telnet] " + " | ".join(events))
            print("----- RENDER -----")
            print(render(clean))
            print("----- REPR -----")
            print(repr(clean))
        elif cmd == "expect":
            # `expect [TIMEOUT] PATTERN` — read until PATTERN appears.
            toks = arg.split(" ", 1)
            if toks and toks[0].replace(".", "", 1).isdigit():
                mw = float(toks[0]); pat = toks[1] if len(toks) > 1 else ""
            else:
                mw = bbs.maxwait; pat = arg
            patb = unescape(pat)
            clean, ok = bbs.read_until(patb, maxwait=mw)
            n += 1
            status = "MATCHED" if ok else f"!! TIMEOUT waiting for {pat!r}"
            print(f"\n===== CAPTURE #{n} (expect {pat!r}: {status}) =====")
            print("----- RENDER -----")
            print(render(clean))
            print("----- REPR -----")
            print(repr(clean))
        elif cmd == "send":
            b = unescape(arg)
            print(f"\n>>> SEND {b!r}")
            bbs.send(b)
        elif cmd == "sendline":
            b = unescape(arg) + b"\r"
            print(f"\n>>> SENDLINE {b!r}")
            bbs.send(b)
        elif cmd == "sleep":
            time.sleep(float(arg))
        else:
            print(f"!! unknown directive: {line}", file=sys.stderr)


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        print(__doc__); sys.exit(2)
    host, port = args[0], int(args[1])
    idle, maxwait, script = 0.8, 12.0, None
    i = 2
    while i < len(args):
        if args[i] == "--idle":
            idle = float(args[i + 1]); i += 2
        elif args[i] == "--maxwait":
            maxwait = float(args[i + 1]); i += 2
        elif args[i] == "--script":
            script = args[i + 1]; i += 2
        else:
            i += 1
    bbs = BBS(host, port, idle=idle, maxwait=maxwait)
    lines = open(script).readlines() if script else sys.stdin.readlines()
    try:
        run_script(bbs, lines)
    finally:
        bbs.close()


if __name__ == "__main__":
    main()
