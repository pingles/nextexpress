#!/usr/bin/env python3
"""Robust AmiExpress (FS-UAE) session driver.

The emulated box has two hazards:
  1. An abruptly-closed telnet socket is NOT detected as carrier-loss under
     FS-UAE bsdsocket emulation, so the node spins forever (pegging the
     emulated CPU). EVERY session MUST end with a clean `G Y` logoff.
  2. Per-prompt latency is high and variable, so we use pattern-based expect
     with generous timeouts, never fixed cadence.

A connection that receives "No nodes available" or is refused has NOT grabbed a
node, so closing it is safe. Only a connection that reached "Successful
connection to node N" holds a node and must be logged off.
"""
import sys
import time
sys.path.insert(0, "/tmp")
from bbsdrive import BBS, strip_iac, render


def connect_until_node(host, port, retries=50, delay=4.0, log=None):
    """Retry until we land on a node; return a live BBS positioned just before
    the graphics prompt. Connections that don't grab a node are closed safely."""
    for attempt in range(retries):
        try:
            bbs = BBS(host, port, idle=1.5, maxwait=8, connect_timeout=8)
        except OSError:
            time.sleep(delay)
            continue
        clean, ok = bbs.read_until(b"graphics", maxwait=10)
        if b"Successful connection to node" in clean and b"graphics" in clean:
            if log is not None:
                log.append("@@@@@ CONNECT @@@@@")
                node = clean.split(b"connection to node ")[1].split(b"\\")[0][:3]
                log.append(f"(landed on node, attempt {attempt})")
                log.append(render(clean))
            return bbs, clean
        # Did not grab a node (no free node / partial) -> safe to close + retry.
        bbs.close()
        time.sleep(delay)
    raise RuntimeError("could not grab a node after %d attempts" % retries)


class Session:
    def __init__(self, host, port, user="sysop", pw="sysop"):
        self.log = []
        self.bbs, banner = connect_until_node(host, port, log=self.log)
        self.user = user
        self.pw = pw

    def cap(self, label, send=None, until=None, idle=1.5, maxwait=20):
        if send is not None:
            self.bbs.send(send)
        if until is not None:
            clean, ok = self.bbs.read_until(until if isinstance(until, bytes)
                                            else until.encode(), maxwait=maxwait)
            status = "MATCHED" if ok else f"TIMEOUT(waiting {until!r})"
        else:
            clean = self.bbs.read_idle(idle=idle, maxwait=maxwait)
            status = "idle"
        self.log.append(f"\n@@@@@ {label} @@@@@ [{status}]")
        if send is not None:
            self.log.append(f">>> SENT {send!r}")
        self.log.append("----- RENDER -----")
        self.log.append(render(clean))
        self.log.append("----- REPR -----")
        self.log.append(repr(clean))
        return clean

    def login(self):
        self.cap("graphics -> A", b"A", until=b"Name:", maxwait=30)
        self.cap("name -> user", self.user.encode() + b"\r", until=b"assword", maxwait=30)
        # password; capture post-login generously (idle)
        return self.cap("password -> POST-LOGIN", self.pw.encode() + b"\r",
                        idle=2.5, maxwait=45)

    def logoff(self):
        """Force a clean logoff so the node frees. Best-effort, idempotent."""
        try:
            self.bbs.send(b"G Y\r")
        except OSError:
            return
        # Read until the server drops carrier (EOF) or a few rounds elapse;
        # answer any stray y/n confirmation with Y.
        for _ in range(6):
            clean = self.bbs.read_idle(idle=1.5, maxwait=12)
            self.log.append("\n@@@@@ LOGOFF round @@@@@")
            self.log.append(render(clean))
            if clean == b"" :
                break
            low = clean.lower()
            if b"no carrier" in low or b"goodbye" in low:
                # one more drain
                self.bbs.read_idle(idle=1.0, maxwait=4)
                break
            if b"(y/n)" in low or b"y/n)?" in low or b"sure" in low or b"abandon" in low:
                try:
                    self.bbs.send(b"Y\r")
                except OSError:
                    break
        self.bbs.close()

    def save(self, path):
        with open(path, "w") as f:
            f.write("\n".join(self.log))
