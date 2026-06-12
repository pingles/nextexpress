# NextScan Interactive Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the F (NextScan) command interactively correct: UTF-8 wire output, true single-key pager hotkeys with per-keystroke echo, and an on-row `[X]` flag marker painted in place.

**Architecture:** Three sequential slices per `designs/2026-06-12-utf8-hotkeys-flagmark-design.md` — D2u re-encodes the wire constants to UTF-8 (`&str` + char-aware column maths + a decode gate); D2b adds a `Terminal::read_key` port and rewrites the More?/ns-confirm/flag prompts as hot-key loops; D2f adds a session-scoped `FlaggedFiles` domain set, a 4-column marker slot in the row renderer, and ANSI cursor repaint. A live-board probe battery precedes D2b to pin the two uncaptured corners.

**Tech Stack:** Rust (tokio, cargo-nextest, cargo-mutants), Python capture harness (`comparison/harness/bbsdrive.py`), FS-UAE Docker reference board.

**House rules (AGENTS.md):** TDD — failing test first, minimal code, `cargo nextest run` from `rust/`, mutation testing per slice, no compile warnings. fmt/clippy run via hooks. Commit style follows `git log` (`Menu:`/`Adapters:`/`Files:`/`Docs:` prefixes). All work is on `main`.

---

## Phase 0 — Probe battery (pins the uncaptured corners before D2b)

### Task 0.1: Write the probe harness script

**Files:**
- Create: `comparison/harness/ae_tierd_probes.py`

- [ ] **Step 1: Write the script** (modeled on `comparison/harness/ae_tierd_aquascan3.py`; uses the same `ae_tierc` helpers — `read_until_any` returns `(clean_bytes, hit_pattern_or_None)` and idles out per `maxwait`):

```python
#!/usr/bin/env python3
"""Tier D probes — the three uncaptured AquaScan corners (design
2026-06-12 §6.1):

  P1: held lone `n` at More?, then bare CR  -> does n+Enter Quit?
  P2: bare LF at a fresh More?              -> LF as a keypress?
  P3: flag prompt fed one byte at a time    -> per-keystroke echo?

Each step logs the EXACT bytes sent and a per-step idle snapshot of the
bytes received, so echo timing is observable (the gap that caused the
D2 echo defect). Every session ends with a clean `G Y` logoff (FS-UAE
node-spin hazard — see ae_tierd_aquascan3.py).
"""
import sys
import os
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ae_tierc import (  # noqa: E402
    LOG, to_menu, to_pattern, connect_until_node, HOST, PORT,
    MENU_SENTINEL, read_until_any, emit,
)

MORE = b"uit:"      # ...(Q)uit:
FLAG = b"to flag:"  # File name(s) to flag:


def snapshot(bbs, label, data, collected, settle=3):
    """Send `data` raw, then read everything that arrives within
    `settle` seconds of idle. Returns the received bytes."""
    bbs.send(data)
    got, _ = read_until_any(bbs, [b"\xde\xad\xbe\xef"], maxwait=settle)
    collected.append(b"<<<sent %r got>>>%s<<<end %s>>>" % (data, got, label.encode()))
    return got


def to_more(bbs, collected):
    bbs.send(b"F 1\r")
    got, hit = read_until_any(bbs, [MORE], maxwait=45)
    collected.append(got)
    assert hit == MORE, "never reached More? prompt"


def recover(bbs, collected):
    for esc in (b"Q", b"\r", b"q\r"):
        bbs.send(esc)
        got, hit = read_until_any(bbs, [MENU_SENTINEL], maxwait=20)
        collected.append(b"<<<recovery %r>>>" % esc + got)
        if hit == MENU_SENTINEL:
            return
    raise RuntimeError("could not recover to menu")


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "/tmp/ae_tierd_probes.txt"
    bbs, banner = connect_until_node(HOST, PORT, log=LOG)
    try:
        to_pattern(bbs, "graphics -> A", b"A\r", b"Name:", maxwait=60)
        to_pattern(bbs, "name -> sysop", b"sysop\r", b"assword", maxwait=40)
        to_menu(bbs, "password -> POST-LOGIN", b"sysop\r", maxwait=120)
        to_menu(bbs, "ensure conf 2 (Amiga, seeded)", b"J 2\r")

        # P1: lone n, idle, then bare CR.
        c = []
        to_more(bbs, c)
        snapshot(bbs, "P1 lone n", b"n", c)
        got = snapshot(bbs, "P1 n then CR", b"\r", c, settle=5)
        emit("P1: held n + Enter", b"n,\\r", b"".join(c),
             "MENU" if MENU_SENTINEL in got else "STILL IN PAGER")
        if MENU_SENTINEL not in got:
            recover(bbs, c)

        # P2: bare LF at a fresh More?.
        c = []
        to_more(bbs, c)
        got = snapshot(bbs, "P2 bare LF", b"\n", c, settle=5)
        emit("P2: bare LF at More?", b"\\n", b"".join(c),
             "MENU" if MENU_SENTINEL in got else "PAGER/STREAMED")
        if MENU_SENTINEL not in got:
            recover(bbs, c)

        # P3: flag entry typed one byte at a time.
        c = []
        to_more(bbs, c)
        bbs.send(b"F")
        got, hit = read_until_any(bbs, [FLAG], maxwait=20)
        c.append(got)
        assert hit == FLAG, "flag prompt never appeared"
        for byte in b"TERMV48":
            snapshot(bbs, "P3 byte %c" % byte, bytes([byte]), c, settle=2)
            time.sleep(0.3)
        snapshot(bbs, "P3 finish", b".LHA\r", c, settle=5)
        emit("P3: flag entry per-byte echo", b"T,E,R,M,V,4,8,.LHA\\r",
             b"".join(c), "DONE")
        recover(bbs, c)

        to_menu(bbs, "clean logoff", b"G Y\r", maxwait=30)
    finally:
        bbs.close()


if __name__ == "__main__":
    main()
```

NOTE for the implementer: before running, open `comparison/harness/ae_tierc.py` and confirm the imported names (`to_menu`, `to_pattern`, `connect_until_node`, `read_until_any`, `emit`, `LOG`, `HOST`, `PORT`, `MENU_SENTINEL`) and the `emit` signature match this usage; `ae_tierd_aquascan3.py` is the working reference. Adjust mechanically if a helper differs (e.g. `bbs.close()` may be `bbs.sock.close()`); also confirm whether `G Y\r` reaches a `Goodbye` rather than `MENU_SENTINEL` and use the same final pattern `ae_tierd_aquascan3.py` uses for its logoff.

- [ ] **Step 2: Commit**

```bash
git add comparison/harness/ae_tierd_probes.py
git commit -m "Harness: Tier D probe battery — the three uncaptured pager corners"
```

### Task 0.2: Run the probes against the reference board, record results

- [ ] **Step 1: Start the board** (boot ~2–3 min):

```bash
docker run -d --name amiexpress-ref -p 127.0.0.1:6023:6023 -e NODE_COUNT=4 \
  -v nextexpress-aros-roms:/opt/aros -v nextexpress-aros-system:/amiga/workbench \
  -v nextexpress-bbs:/amiga/bbs nextexpress/amiexpress-fsuae:latest
```

(If the container already exists: `docker start amiexpress-ref`. If connections are refused after 4 min, check `docker logs amiexpress-ref`.)

- [ ] **Step 2: Run** `python3 comparison/harness/ae_tierd_probes.py comparison/transcripts/ae_tierd_probes.txt` — expect three `emit` blocks and a clean logoff. If a node spin-waits (CPU pegged, no reply), restart the container and re-run; never leave a session unclosed.

- [ ] **Step 3: Read the transcript and record the three verdicts** in `comparison/evidence-tierD/live-observations.md` under a new `## Probe battery 2026-06-12` heading: P1 (does `n`+CR produce `\x08 \x08Quit` / plain `Quit` / continue?), P2 (does bare LF continue, quit, or nothing?), P3 (does each typed byte echo back within the idle window — i.e. is the door's flag line read per-keystroke echoing?). Quote the exact byte evidence per verdict. Also add the `## Methodology blind spots` section listing what flat captures cannot show (echo timing, charset, cross-window effects) per design §9.

- [ ] **Step 4: If a verdict contradicts the design defaults** (P1 default: `\x08 \x08Quit\r\n` + quit; P2 default: Enter≡continue; P3 default: per-key echo), update `designs/2026-06-12-utf8-hotkeys-flagmark-design.md` §4 accordingly — the board wins. Tasks 2.4/2.5 below consume these verdicts.

- [ ] **Step 5: Stop the board** (`docker stop amiexpress-ref`) **and commit**

```bash
git add comparison/transcripts/ae_tierd_probes.txt comparison/evidence-tierD/live-observations.md designs/2026-06-12-utf8-hotkeys-flagmark-design.md
git commit -m "Docs: probe battery pins held-n+Enter, bare-LF and flag-echo corners"
```

---

## Phase 1 — Slice D2u: UTF-8 wire

### Task 1.1: Re-encode the four Latin-1 constants as UTF-8 `&str`

**Files:**
- Modify: `rust/src/app/menu_flow/file_list/wire.rs` (constants at :19-26, :56-61, :163-186; module doc :1-12; `separator_block` :69-79; tests :300-346, :425-446)
- Modify: `rust/src/app/menu_flow/file_list/mod.rs:48` and `:108` (`HELP_SCREEN`/`HELP_BANNER` call sites gain `.as_bytes()`)

- [ ] **Step 1: Write the failing tests** — in `wire.rs` `mod tests`, add a char-aware width helper and a decode gate, and re-pin the separator/banner expectations to UTF-8:

```rust
    /// Visible columns of a UTF-8 string: every char outside `ESC[..m`
    /// SGR runs is one column (all NextScan glyphs are single-cell).
    fn visible_width_str(s: &str) -> usize {
        let mut width = 0;
        let mut rest = s;
        while let Some(c) = rest.chars().next() {
            if c == '\x1b' {
                let end = rest.find('m').expect("SGR sequence terminated");
                rest = &rest[end + 1..];
            } else {
                width += 1;
                rest = &rest[c.len_utf8()..];
            }
        }
        width
    }

    #[test]
    fn all_wire_output_is_valid_utf8() {
        // Encoding policy (AGENTS.md "Wire encoding"): the NextExpress
        // wire is valid UTF-8. The art/© constants are &str by type;
        // this gates the assembled byte paths that splice them.
        assert!(String::from_utf8(separator_block("01-15-26").concat()).is_ok());
        assert!(std::str::from_utf8(HELP_SCREEN.as_bytes()).is_ok());
        assert!(std::str::from_utf8(HELP_BANNER.as_bytes()).is_ok());
    }
```

Then change `separator_block_carries_the_file_date_in_the_second_line` (wire.rs:327-346) to expect the UTF-8 glyphs:

```rust
        let block = separator_block("01-15-26");
        assert_eq!(block.len(), 4);
        assert_eq!(block[0], b"\x1b[0m".to_vec());
        let mut line_a = b"\x1b[0m".to_vec();
        line_a.extend_from_slice(&[b' '; 44]);
        line_a.extend_from_slice("_¸,ø*¤°¬°¤*ø,¸_¸,ø*¤°¬¬°¤*ø,¸_".as_bytes());
        assert_eq!(block[1], line_a);
        let mut line_b = b"\x1b[0m".to_vec();
        line_b.extend_from_slice(&[b' '; 6]);
        line_b.extend_from_slice("¸,ø*¤°¬¯¬°¤*ø,¸_¸,ø*¤°¬°¤*ø, 01-15-26".as_bytes());
        assert_eq!(block[2], line_b);
        assert_eq!(block[3], b"\x1b[0m".to_vec());
```

And `help_banner_swaps_brand_and_copyright_and_holds_width` (wire.rs:313-324) to:

```rust
        assert_eq!(
            HELP_BANNER,
            "\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m",
        );
        assert_eq!(visible_width_str(HELP_BANNER), 79);
        assert_eq!(visible_width_str(HELP_BANNER), visible_width(AQUASCAN_HELP_BANNER));
```

(`AQUASCAN_HELP_BANNER` stays a Latin-1 `&[u8]` capture reference and keeps the byte-based `visible_width` — one column per Latin-1 byte is correct for it.)

- [ ] **Step 2: Run to verify failure**: `cd rust && cargo nextest run file_list` — expect type errors/mismatches on `HELP_BANNER`/`SEPARATOR_ART_*`.

- [ ] **Step 3: Implement** — in `wire.rs` change the four constants to `&str` (glyphs via `\u{}` per SLICES.md house style) and rewrite the module doc paragraph (:6-12) to state the new policy ("All output is valid UTF-8 — encoding policy in AGENTS.md; the legacy single Latin-1 art/© bytes are re-encoded, recorded in COMMAND_PARITY.md"):

```rust
pub(super) const HELP_BANNER: &str =
    "\x1b[0m\x1b[34m--[ \x1b[36mNextScan \x1b[34m]----------------------------------[ \x1b[36mCopyright \u{a9} 2026 NextScan \x1b[34m]--\x1b[0m";

/// Separator art line A motif (44-space indent) — the AquaScan wave,
/// `_¸,ø*¤°¬°¤*ø,¸_…`, re-encoded UTF-8.
const SEPARATOR_ART_A: &str =
    "_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_";

/// Separator art line B motif (6-space indent, date appended).
const SEPARATOR_ART_B: &str =
    "\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{af}\u{ac}\u{b0}\u{a4}*\u{f8},\u{b8}_\u{b8},\u{f8}*\u{a4}\u{b0}\u{ac}\u{b0}\u{a4}*\u{f8},";
```

In `separator_block`, the two `extend_from_slice(SEPARATOR_ART_X)` calls become `extend_from_slice(SEPARATOR_ART_X.as_bytes())`. `HELP_SCREEN` becomes `&str` with `\u{a9}` in its banner line (rest of the literal unchanged — it is ASCII). In `mod.rs:48` and `:108`: `self.terminal.write(wire::HELP_SCREEN.as_bytes())` / `wire::HELP_BANNER.as_bytes()`.

- [ ] **Step 4: Fix the remaining Latin-1 literals in test expectations** — `grep -rn '\\xb8\|\\xa9\|\\xf8' rust/src rust/tests`. Expected hits: wire.rs tests (done in Step 1), `rust/tests/tierd_file_list_smoke.rs:40-82` capture-derived heads/tails (re-encode each `\xNN` art/© byte to the UTF-8 glyph in the literal, e.g. `"Copyright \u{a9} 2026"` and the wave strings as in Step 1), and any `file_list/mod.rs` test literals. The `AQUASCAN_*` reference constants are the only Latin-1 bytes that remain, and only in `wire.rs` tests.

- [ ] **Step 5: Run the full suite**: `cd rust && cargo nextest run` — expect PASS. `cargo build` — no warnings.

- [ ] **Step 6: Commit**

```bash
git add rust/src/app/menu_flow/file_list/ rust/tests/tierd_file_list_smoke.rs
git commit -m "Menu: NextScan wire goes UTF-8 — art and © re-encoded from Latin-1 (slice D2u)"
```

### Task 1.2: End-to-end UTF-8 gate in the telnet smoke

**Files:**
- Modify: `rust/tests/tierd_file_list_smoke.rs`

- [ ] **Step 1: Write the failing-by-construction gate** (it passes immediately after Task 1.1 — write it anyway as the permanent regression gate; verify it FAILS if you temporarily revert one wire.rs constant to a Latin-1 byte literal, then restore):

```rust
#[tokio::test]
async fn utf8_gate_every_session_byte_decodes() {
    // Encoding policy (AGENTS.md): the wire is valid UTF-8. Drive the
    // full F surface — listing, pager, help — and assert the entire
    // received stream decodes. Any future capture-pinned constant
    // that re-introduces raw Latin-1 bytes fails here.
    let addr = spawn_runtime().await;
    let mut stream = login(addr).await;
    let mut all = Vec::new();
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"F ?").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    write_line(&mut stream, b"F A NS").await;
    all.extend(drain_until(&mut stream, b"mins. left): ").await);
    assert!(
        std::str::from_utf8(&all).is_ok(),
        "session stream contains non-UTF-8 bytes: {:?}",
        String::from_utf8_lossy(&all)
    );
    end_session(&mut stream).await;
}
```

(Adapt the two helper names to the file's existing ones — `spawn_runtime`/`login` per `tierd_file_list_smoke.rs:280-289`; keep its exact login sentinel strings.)

- [ ] **Step 2: Run**: `cd rust && cargo nextest run utf8_gate` — PASS (and the temporary-revert check above showed it can fail).

- [ ] **Step 3: Commit**

```bash
git add rust/tests/tierd_file_list_smoke.rs
git commit -m "Tests: UTF-8 wire gate — the full F session stream must decode (slice D2u)"
```

### Task 1.3: D2u docs + policy + mutants

**Files:**
- Modify: `AGENTS.md`, `COMMAND_PARITY.md`, `designs/NEXTSCAN.md` (:11, :184, :200), `SLICES.md` (:303-309 area)

- [ ] **Step 1: AGENTS.md** — add a `## Wire encoding` section after `## Style Guidelines`:

```markdown
## Wire encoding

The NextExpress wire is **valid UTF-8, always**. The legacy board emits
ISO-8859-1 (Amiga) bytes; when porting captured output, re-encode each
high-bit byte to the same code point in UTF-8 (`\xa9` → `\u{a9}`) and
record the departure as a COMMAND_PARITY.md row. Never emit raw bytes
≥ 0x80 outside a valid UTF-8 sequence — the e2e UTF-8 gate
(`tierd_file_list_smoke.rs::utf8_gate_every_session_byte_decodes`)
enforces this. Rust consts carrying re-encoded glyphs are `&str`.
```

- [ ] **Step 2: COMMAND_PARITY.md** — in the legend (:34 area): add that **encoding and interaction divergences are at minimum BEHAVIOURAL, never COSMETIC**; add a Tier D row: NextScan art/© emitted UTF-8 vs legacy single Latin-1 bytes (deliberate policy, AGENTS.md "Wire encoding"); update the Tier A–C © rows (:78, :204) to point at the policy (now resolved, not merely "worth flagging").
- [ ] **Step 3: NEXTSCAN.md** — amend :11 and :184 (the `&[u8]` Latin-1 mandate) and :200 to defer to the new policy; cross-reference the 2026-06-12 design doc. **SLICES.md** :303-309: note the policy section now owns this rule.
- [ ] **Step 4: Mutants**: `cd rust && cargo mutants --file src/app/menu_flow/file_list/wire.rs` — expect 0 missed; add/strengthen tests if any survive (or document equivalence in the commit message).
- [ ] **Step 5: Commit**

```bash
git add AGENTS.md COMMAND_PARITY.md designs/NEXTSCAN.md SLICES.md
git commit -m "Docs: wire-encoding policy — UTF-8 always, parity tags fixed (slice D2u)"
```

---

## Phase 2 — Slice D2b: true hotkeys

### Task 2.1: `KeyEvent`/`KeyRead` + `Terminal::read_key` port

**Files:**
- Modify: `rust/src/app/terminal.rs`
- Modify: `rust/src/app/colour_terminal.rs` (delegate)

- [ ] **Step 1: Write the failing test** — in `colour_terminal.rs` `mod tests` (proves the decorator delegates and the default exists):

```rust
    #[tokio::test]
    async fn colour_terminal_delegates_read_key() {
        use crate::app::terminal::{KeyEvent, KeyRead};
        // CaptureTerminal inherits the trait default (Eof); the
        // decorator must pass the call through rather than answer
        // itself — extend CaptureTerminal with a scripted key to
        // observe the delegation.
        struct OneKey;
        impl Terminal for OneKey {
            type Error = std::convert::Infallible;
            fn write<'a>(&'a mut self, _b: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
                Box::pin(async { Ok(()) })
            }
            fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
                Box::pin(async { Ok(()) })
            }
            fn read_line(
                &mut self,
                _echo: TerminalEcho,
                _timeout: Duration,
            ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
                Box::pin(async { Ok(TerminalRead::Eof) })
            }
            fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
                Box::pin(async { Ok(KeyRead::Key(KeyEvent::Char(b'q'))) })
            }
        }
        let mut term = ColourTerminal::new(OneKey, true);
        let key = term.read_key(Duration::from_secs(1)).await.unwrap();
        assert_eq!(key, KeyRead::Key(KeyEvent::Char(b'q')));
    }
```

- [ ] **Step 2: Run to verify failure**: `cd rust && cargo nextest run colour_terminal` — FAIL: `KeyEvent`/`KeyRead`/`read_key` not defined.
- [ ] **Step 3: Implement** — in `terminal.rs` after `TerminalRead`:

```rust
/// A single keystroke read from the terminal in hot-key mode
/// (slice D2b — the AquaScan pager prompts act per key, no Enter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyEvent {
    /// A printable ASCII key (0x20..=0x7E).
    Char(u8),
    /// Enter — CR with an optional LF/NUL trailer. A bare LF is NOT
    /// Enter: the board swallows it entirely (probe P2,
    /// `ae_tierd_probes.txt:140-175`), so the adapter emits no event.
    Enter,
    /// Backspace (0x08) or DEL (0x7F).
    Backspace,
    /// Anything else: other control bytes, bytes ≥ 0x80, or one
    /// swallowed `ESC[…` sequence (an arrow press is ONE event, so it
    /// cannot fire three pager verbs).
    Other,
}

/// Result of a bounded single-key read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyRead {
    /// One keystroke arrived.
    Key(KeyEvent),
    /// The peer disconnected cleanly.
    Eof,
    /// No key arrived before the supplied timeout elapsed.
    IdleTimedOut,
}
```

and on the trait, after `read_line`:

```rust
    /// Reads one keystroke in hot-key mode. The adapter echoes
    /// NOTHING — the caller owns every user-visible byte (the door
    /// echoes verbs itself, `amiexpress/express.e:5154-5179` readChar).
    ///
    /// The default returns `Eof` so line-only test fakes need no
    /// override; transports and decorators MUST override (gated by
    /// the keystroke smoke in `tierd_hotkey_smoke.rs`).
    fn read_key(&mut self, _timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        Box::pin(async { Ok(KeyRead::Eof) })
    }
```

In `colour_terminal.rs`, add to the `impl Terminal for ColourTerminal<T>` (cursor-repaint CSI must reach non-ANSI clients never — but `read_key` is input, it simply delegates):

```rust
    fn read_key(&mut self, timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        self.inner.read_key(timeout)
    }
```

(Import `KeyRead` in `colour_terminal.rs`'s use line.)

- [ ] **Step 4: Run**: `cd rust && cargo nextest run colour_terminal` — PASS; full `cargo nextest run` still green.
- [ ] **Step 5: Commit**

```bash
git add rust/src/app/terminal.rs rust/src/app/colour_terminal.rs
git commit -m "App: Terminal::read_key port — hot-key events for the pager (slice D2b)"
```

### Task 2.2: Telnet adapter `read_telnet_key`

**Files:**
- Modify: `rust/src/adapters/telnet_line.rs`
- Modify: `rust/src/adapters/telnet_listener.rs` (`TelnetTerminal::read_key` at :163-194)

- [ ] **Step 1: Write the failing adapter tests** in `telnet_line.rs` `mod tests`:

```rust
    use crate::app::terminal::KeyEvent;

    #[tokio::test]
    async fn read_key_maps_printables_enter_variants_and_backspace() {
        // Bare LF is swallowed with no event — the board drops it
        // entirely (probe P2, ae_tierd_probes.txt:140-175) — so the
        // lone `\n` below yields nothing and the next event after `x`
        // is the Backspace.
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"n\r\x00Q\r\nx\n\x08").await.unwrap();
        let mut pushback = None;
        let mut keys = Vec::new();
        for _ in 0..6 {
            keys.push(read_telnet_key(&mut server, &mut pushback).await.unwrap().unwrap());
        }
        assert_eq!(
            keys,
            vec![
                KeyEvent::Char(b'n'),
                KeyEvent::Enter, // CR NUL
                KeyEvent::Char(b'Q'),
                KeyEvent::Enter, // CR LF
                KeyEvent::Char(b'x'),
                KeyEvent::Backspace, // the bare LF before it: no event
            ]
        );
    }

    #[tokio::test]
    async fn read_key_swallows_a_csi_sequence_as_one_event() {
        let (mut server, mut client) = connected_pair().await;
        client.write_all(b"\x1b[Ay").await.unwrap();
        let mut pushback = None;
        let first = read_telnet_key(&mut server, &mut pushback).await.unwrap().unwrap();
        let second = read_telnet_key(&mut server, &mut pushback).await.unwrap().unwrap();
        assert_eq!(first, KeyEvent::Other, "arrow = one event");
        assert_eq!(second, KeyEvent::Char(b'y'));
    }

    #[tokio::test]
    async fn read_key_skips_iac_and_echoes_nothing() {
        use tokio::io::AsyncReadExt;
        let (mut server, mut client) = connected_pair().await;
        client.write_all(&[0xFF, 0xFD, 0x01, b'n']).await.unwrap();
        let mut pushback = None;
        let key = read_telnet_key(&mut server, &mut pushback).await.unwrap().unwrap();
        assert_eq!(key, KeyEvent::Char(b'n'));
        drop(server);
        let mut echoed = Vec::new();
        client.read_to_end(&mut echoed).await.unwrap();
        assert_eq!(echoed, b"", "key reads must write zero bytes");
    }
```

- [ ] **Step 2: Run to verify failure**: `cd rust && cargo nextest run telnet_line` — FAIL: `read_telnet_key` undefined.
- [ ] **Step 3: Implement** in `telnet_line.rs` — first extract the IAC-skipping block of `read_telnet_line` (:71-95) into a shared helper, keeping `read_telnet_line` green:

```rust
/// Consumes the remainder of an IAC sequence whose 0xFF has already
/// been read: 3-byte negotiations, and `SB … IAC SE` subnegotiation.
/// `Ok(false)` = EOF mid-sequence.
async fn skip_iac(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<bool> {
    let Some(cmd) = read_one(stream, pushback).await? else {
        return Ok(false);
    };
    if (0xFB..=0xFE).contains(&cmd) {
        let _ = read_one(stream, pushback).await?;
    } else if cmd == 0xFA {
        loop {
            let Some(b1) = read_one(stream, pushback).await? else {
                return Ok(false);
            };
            if b1 == 0xFF {
                let Some(b2) = read_one(stream, pushback).await? else {
                    return Ok(false);
                };
                if b2 == 0xF0 {
                    break;
                }
            }
        }
    }
    Ok(true)
}
```

then the key reader:

```rust
/// Reads one keystroke: IAC-aware, echoes nothing (hot-key echo is
/// the handler's job — the door model, `express.e:5154-5179`).
/// CR (with optional LF/NUL trailer) and bare LF are one `Enter`;
/// a buffered `ESC[…` sequence is swallowed into one `Other` so an
/// arrow press cannot fire several pager verbs. `Ok(None)` = EOF.
pub(crate) async fn read_telnet_key(
    stream: &mut TcpStream,
    pushback: &mut Option<u8>,
) -> io::Result<Option<KeyEvent>> {
    loop {
        let Some(b) = read_one(stream, pushback).await? else {
            return Ok(None);
        };
        match b {
            0xFF => {
                if !skip_iac(stream, pushback).await? {
                    return Ok(None);
                }
            }
            b'\r' => {
                try_consume_cr_trailer(stream, pushback)?;
                return Ok(Some(KeyEvent::Enter));
            }
            // Bare LF: the board swallows it — no event, not even
            // Other (which would continue the pager). Probe P2,
            // ae_tierd_probes.txt:140-175.
            b'\n' => {}
            0x08 | 0x7F => return Ok(Some(KeyEvent::Backspace)),
            0x1b => {
                swallow_buffered_csi(stream, pushback)?;
                return Ok(Some(KeyEvent::Other));
            }
            b if (0x20..=0x7E).contains(&b) => return Ok(Some(KeyEvent::Char(b))),
            _ => return Ok(Some(KeyEvent::Other)),
        }
    }
}

/// Best-effort, non-blocking swallow of an already-buffered CSI
/// remainder (`[ <params> <final>`): a full arrow/function sequence
/// arrives in one packet, so its bytes are queued; a lone ESC press
/// has nothing queued and is left alone. Bounded at 8 bytes.
fn swallow_buffered_csi(stream: &mut TcpStream, pushback: &mut Option<u8>) -> io::Result<()> {
    let mut byte = [0u8; 1];
    match stream.try_read(&mut byte) {
        Ok(n) if n > 0 && byte[0] == b'[' => {}
        Ok(n) if n > 0 => {
            *pushback = Some(byte[0]);
            return Ok(());
        }
        Ok(_) => return Ok(()), // EOF
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(e) => return Err(e),
    }
    for _ in 0..8 {
        match stream.try_read(&mut byte) {
            Ok(n) if n > 0 => {
                if (0x40..=0x7E).contains(&byte[0]) {
                    return Ok(()); // final byte — sequence complete
                }
                // parameter/intermediate byte: keep consuming
            }
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
```

(import `KeyEvent` from `crate::app::terminal`). Replace `read_telnet_line`'s inline IAC block with `if !skip_iac(stream, pushback).await? { return Ok(None); }`.

In `telnet_listener.rs`, add to `impl Terminal for TelnetTerminal<'_>` (mirror of `read_line` at :174-193):

```rust
    fn read_key(&mut self, timeout: Duration) -> TerminalFuture<'_, KeyRead, Self::Error> {
        Box::pin(async move {
            match tokio::time::timeout(
                timeout,
                read_telnet_key(self.stream, &mut self.pushback),
            )
            .await
            {
                Ok(result) => match result? {
                    Some(key) => Ok(KeyRead::Key(key)),
                    None => Ok(KeyRead::Eof),
                },
                Err(_elapsed) => Ok(KeyRead::IdleTimedOut),
            }
        })
    }
```

(import `read_telnet_key` and `KeyRead`).

- [ ] **Step 4: Run**: `cd rust && cargo nextest run telnet` — PASS; full suite green; `cargo build` no warnings.
- [ ] **Step 5: Commit**

```bash
git add rust/src/adapters/telnet_line.rs rust/src/adapters/telnet_listener.rs
git commit -m "Adapters: read_telnet_key — IAC-aware single-key reads, CSI swallowed whole (slice D2b)"
```

### Task 2.3: Key-scripted test fake for the pager tests

**Files:**
- Modify: `rust/src/app/menu_flow/file_list/mod.rs` (test module, `CaptureTerminal` at :460)

- [ ] **Step 1:** Extend the file_list test fake with a key queue (the other eight `impl Terminal` fakes inherit the trait default and stay untouched):

```rust
    // inside the existing CaptureTerminal struct definition, add:
    keys: VecDeque<crate::app::terminal::KeyRead>,

    // inside `impl Terminal for CaptureTerminal`, add:
    fn read_key(
        &mut self,
        _timeout: Duration,
    ) -> TerminalFuture<'_, crate::app::terminal::KeyRead, Self::Error> {
        let key = self
            .keys
            .pop_front()
            .unwrap_or(crate::app::terminal::KeyRead::Eof);
        Box::pin(async move { Ok(key) })
    }
```

and a builder helper next to the fake's constructor: `fn with_keys(reads: Vec<TerminalRead>, keys: Vec<KeyRead>) -> Self` (match the existing constructor's shape; default `keys: VecDeque::new()` in the current constructor so existing tests compile unchanged).

- [ ] **Step 2: Run**: `cd rust && cargo nextest run file_list` — still green (no behaviour change yet).
- [ ] **Step 3: Commit**

```bash
git add rust/src/app/menu_flow/file_list/mod.rs
git commit -m "Tests: file_list fake terminal learns scripted key events (slice D2b)"
```

### Task 2.4: Rewrite `scan_more_prompt` + ns-confirm as hot-key loops

**Files:**
- Modify: `rust/src/app/menu_flow/file_list/mod.rs` (:266-371 `scan_more_prompt`; tests)
- Modify: `rust/src/app/menu_flow/mod.rs` (add a `read_key` helper next to `read_prompted` at :329)

**PRE-REQUISITE: the Task 0.2 probe verdicts are in and ALREADY encoded below.** P1 (`ae_tierd_probes.txt:100-138`): held-`n`+Enter quits with the CR echoed as `\r\n` and then the normal exit tail — NO `Quit` word, NO `BS SP BS` (the held `n` stays on the prompt line). P2 (`:140-175`): bare LF is swallowed by the adapter (Task 2.2), so it never reaches this loop.

- [ ] **Step 1: Write the failing tests** (replace the line-based pager tests' input scripting; key sequences instead of `TerminalRead::Line` verbs). The four load-bearing cases:

```rust
    use crate::app::terminal::{KeyEvent, KeyRead};

    fn key(c: u8) -> KeyRead {
        KeyRead::Key(KeyEvent::Char(c))
    }

    #[tokio::test]
    async fn q_at_more_quits_on_a_single_keypress() {
        // Bare Q byte, no terminator: door echoes the word and exits
        // (ae_tierd_aquascan3.txt:321, sent as a bare key by the
        // harness recovery).
        // Drive F 1 with enough seeded files to hit the More? page
        // boundary, queue ONLY [key(b'Q')] as key input, and assert
        // the output ends prompt + "Quit\r\n" + LISTING_EXIT_TAIL.
    }

    #[tokio::test]
    async fn lone_n_echoes_holds_then_enter_quits() {
        // Probe P1 (comparison/transcripts/ae_tierd_probes.txt:100-138):
        // n echoes immediately; Enter quits WITHOUT the Quit word and
        // WITHOUT BS-SP-BS — the CR echoes "\r\n" and the exit tail
        // follows directly (byte-identical to Q's exit with "Quit"
        // replaced by the echoed "\r\n").
        // Keys: [key(b'n'), KeyRead::Key(KeyEvent::Enter)].
        // Assert output contains prompt + "n" then "\r\n" +
        // LISTING_EXIT_TAIL, and does NOT contain "Quit" after the n.
    }

    #[tokio::test]
    async fn held_n_then_other_key_erases_and_runs_the_verb() {
        // ae_tierd_aquascan4.txt U1: n … Q -> "\x08 \x08" + "Quit".
        // Keys: [key(b'n'), key(b'Q')].
        // Assert "n" then "\x08 \x08Quit\r\n".
    }

    #[tokio::test]
    async fn n_then_s_opens_the_nonstop_confirm_and_y_goes_nonstop() {
        // ae_tierd_aquascan3.txt:154-156 + U3: ns = two bare keys.
        // Keys: [key(b'n'), key(b's'), key(b'Y')].
        // Assert: "n", then the 69-space overprint clear, then
        // NS_CONFIRM_PROMPT, then the overprint clear again, then the
        // listing streams to the end with no further More?.
    }
```

Write them concretely against the existing test scaffolding (the current tests at mod.rs:660-720 show how a listing is driven and output asserted — keep the same `AppServices`/seed setup and byte-expectation style, swapping line scripts for key scripts). Also re-pin the existing More?-related tests (`Y` resumes via overprint, `C` form-feeds, `?` help+redraw, unknown key resumes, flag prompts open) to key scripting: `Y`⇒`[key(b'Y')]`, Enter⇒`[KeyRead::Key(KeyEvent::Enter)]` (continue), etc. Delete the now-meaningless "lone n held across two LINE reads" expectations.

- [ ] **Step 2: Run to verify failure**: `cd rust && cargo nextest run file_list` — new tests FAIL (pager still line-based).
- [ ] **Step 3: Implement.** In `menu_flow/mod.rs` add beside `read_prompted` (:329):

```rust
    async fn read_key(&mut self) -> Result<crate::app::terminal::KeyRead, T::Error> {
        let timeout = self.services.session_policy.input_timeout();
        self.terminal.flush().await?;
        self.terminal.read_key(timeout).await
    }
```

Replace `scan_more_prompt` (file_list/mod.rs:284-371) with the hot-key loop:

```rust
    /// One `More?` interaction — true hotkeys (slice D2b): every verb
    /// acts on a single keypress with door-style immediate echo
    /// (`ae_tierd_aquascan3.txt` S2/S4-S7, `ae_tierd_aquascan4.txt`
    /// U1-U3, probe battery `ae_tierd_probes.txt` P1/P2).
    async fn scan_more_prompt(&mut self, state: &mut ScanState) -> Result<ScanFlow, T::Error> {
        self.terminal.write(wire::MORE_PROMPT).await?;
        let mut held_n = false;
        loop {
            let read = self.read_key().await?;
            let crate::app::terminal::KeyRead::Key(mut key) = read else {
                // Carrier loss / idle at the pager aborts the listing.
                return Ok(ScanFlow::Quit);
            };
            if held_n {
                held_n = false;
                match key {
                    KeyEvent::Char(b's' | b'S') => {
                        // `ns`: wipe the prompt line (the echoed n
                        // included) and confirm (U3).
                        self.terminal.write(&more_overprint_clear()).await?;
                        self.terminal.write(wire::NS_CONFIRM_PROMPT).await?;
                        let confirm = self.read_key().await?;
                        let crate::app::terminal::KeyRead::Key(confirm) = confirm else {
                            return Ok(ScanFlow::Quit);
                        };
                        self.terminal.write(&more_overprint_clear()).await?;
                        if matches!(confirm, KeyEvent::Char(b'y' | b'Y')) {
                            state.non_stop = true;
                            return Ok(ScanFlow::Continue);
                        }
                        self.terminal.write(wire::MORE_PROMPT).await?;
                        continue;
                    }
                    KeyEvent::Enter => {
                        // Probe P1 (ae_tierd_probes.txt:100-138):
                        // Enter after a held n quits with the CR
                        // echoed as \r\n and the exit tail following
                        // directly — no Quit word, no BS-SP-BS; the
                        // held n stays on the prompt line.
                        self.terminal.write(b"\r\n").await?;
                        return Ok(ScanFlow::Quit);
                    }
                    other => {
                        // The next key erases the held n, then runs
                        // as its own verb (U1).
                        self.terminal.write(b"\x08 \x08").await?;
                        key = other;
                    }
                }
            }
            match key {
                KeyEvent::Char(b'n' | b'N') => {
                    // Ambiguous N/ns prefix: echo and hold for the
                    // next key (U1; mid-list and post-End identical).
                    self.terminal.write(b"n").await?;
                    self.terminal.flush().await?;
                    held_n = true;
                }
                KeyEvent::Char(b'q' | b'Q') => {
                    self.terminal.write(b"Quit\r\n").await?;
                    return Ok(ScanFlow::Quit);
                }
                KeyEvent::Char(b'c' | b'C') => {
                    self.terminal.write(b"\r\x0c").await?;
                    return Ok(ScanFlow::Continue);
                }
                KeyEvent::Char(b'f' | b'F') | KeyEvent::Char(b'r' | b'R') => {
                    let by_name = matches!(key, KeyEvent::Char(b'f' | b'F'));
                    let prompt: &[u8] = if by_name {
                        wire::FLAG_BY_NAME_PROMPT
                    } else {
                        wire::FLAG_BY_NUMBER_PROMPT
                    };
                    self.terminal.write(&more_overprint_clear()).await?;
                    self.terminal.write(prompt).await?;
                    let Some(_entry) = self.read_flag_entry().await? else {
                        return Ok(ScanFlow::Quit);
                    };
                    // Read-and-discarded until slice D2f wires
                    // FlaggedFiles; the captured exchange is silent
                    // (S4/S5).
                    self.terminal.write(&flag_overprint_clear()).await?;
                    self.terminal.write(wire::MORE_PROMPT).await?;
                }
                KeyEvent::Char(b'?') => {
                    self.terminal.write(wire::PAUSE_HELP).await?;
                    let page = state.page.clone();
                    for line in &page {
                        self.terminal.write(line).await?;
                        self.terminal.write(b"\r\n").await?;
                    }
                    self.terminal.write(wire::MORE_PROMPT).await?;
                }
                _ => {
                    // Y, Enter (probe P2: ≡ continue), Space, unknown
                    // keys: the captured overprint resume.
                    self.terminal.write(&more_overprint_clear()).await?;
                    return Ok(ScanFlow::Continue);
                }
            }
        }
    }
```

and add the hot-key flag-entry collector below it:

```rust
    /// Hot-key line collector for the flag prompts: each printable
    /// echoes as it arrives (probe P3 — the door's flag read echoes
    /// per keystroke), Backspace erases with BS-SP-BS, and Enter
    /// finishes WITHOUT a terminator echo (the captured exchange has
    /// no CRLF before the 79-space overprint, S4). `None` = carrier
    /// loss / idle timeout.
    async fn read_flag_entry(&mut self) -> Result<Option<String>, T::Error> {
        let mut entry: Vec<u8> = Vec::new();
        loop {
            self.terminal.flush().await?;
            let read = self.read_key().await?;
            let crate::app::terminal::KeyRead::Key(key) = read else {
                return Ok(None);
            };
            match key {
                KeyEvent::Enter => {
                    return Ok(Some(String::from_utf8_lossy(&entry).into_owned()))
                }
                KeyEvent::Backspace => {
                    if entry.pop().is_some() {
                        self.terminal.write(b"\x08 \x08").await?;
                    }
                }
                KeyEvent::Char(b)
                    if entry.len() < crate::app::input_limits::MAX_TERMINAL_LINE_BYTES =>
                {
                    entry.push(b);
                    self.terminal.write(&[b]).await?;
                }
                _ => {}
            }
        }
    }
```

Import `KeyEvent` in file_list/mod.rs's use list. NOTE the MORE? prompt is now written explicitly at loop entry and after F/R/?/declined-ns — delete the old `show_prompt` plumbing; `read_prompted(MORE_PROMPT, Silent)` and the post-hoc `write(entry)` echo disappear.

- [ ] **Step 4: Run**: `cd rust && cargo nextest run file_list` — new tests PASS; fix any stale expectations the rewrite exposed (every change must trace to a capture or probe line — cite it in the test comment).
- [ ] **Step 5: Full suite + build**: `cd rust && cargo nextest run && cargo build` — green, no warnings.
- [ ] **Step 6: Commit**

```bash
git add rust/src/app/menu_flow/file_list/mod.rs rust/src/app/menu_flow/mod.rs
git commit -m "Menu: More?/ns/flag prompts go true hotkey — echo on keypress, act without Enter (slice D2b)"
```

### Task 2.5: Delete `TerminalEcho::Silent` and its plumbing

**Files:**
- Modify: `rust/src/app/terminal.rs` (:24-28), `rust/src/adapters/telnet_line.rs` (:36-41, :105-107, :111-113, :122-124, :137-139, tests :199-246), `rust/src/adapters/telnet_listener.rs` (:196-204)

- [ ] **Step 1:** `grep -rn "Silent" rust/src rust/tests` — after Task 2.4 the only hits must be the enum variants, the `EchoMode` match arms, and the two `telnet_line.rs` silence tests. If a handler still uses it, STOP — that's a missed conversion, fix it first.
- [ ] **Step 2:** Remove `TerminalEcho::Silent`, `EchoMode::Silent` (and its `From` arm at telnet_listener.rs:201), the `if echo != EchoMode::Silent` guards (echo unconditionally in the two modes that remain), the `EchoMode::Silent => continue` arm, and the two `silent_mode_*` tests (their job — pinning handler-owned echo — is superseded by the hot-key tests).
- [ ] **Step 3: Run**: `cd rust && cargo nextest run && cargo build` — green, no warnings (the compiler finds any straggler).
- [ ] **Step 4: Commit**

```bash
git add rust/src/app/terminal.rs rust/src/adapters/
git commit -m "App: TerminalEcho::Silent retired — hotkeys made handler-replayed echo obsolete (slice D2b)"
```

### Task 2.6: Keystroke-granular telnet smoke — the shape that was structurally missing

**Files:**
- Create: `rust/tests/tierd_hotkey_smoke.rs`

- [ ] **Step 1: Write the smoke** (in-process listener per AGENTS.md §e2e; copy the `spawn_runtime`/`login`/`write_line`/`drain_until`/`contains`/`end_session` helpers from `tierd_file_list_smoke.rs:280-324` — subagents read tasks in isolation, so copy, don't import):

```rust
//! Character-mode client smoke for the NextScan hotkey pager
//! (slice D2b). Sends ONE byte at a time and asserts the echo/action
//! arrives BEFORE any terminator is sent — the interaction shape the
//! capture-replay suite structurally missed (RCA 2026-06-11).

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// [copy of spawn_runtime / login / write_line / drain_until / contains
//  from tierd_file_list_smoke.rs goes here]

/// Reads whatever arrives within `window` of idle — the smoke's
/// keystroke-granular observation primitive.
async fn read_for(stream: &mut TcpStream, window: Duration) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    while let Ok(Ok(n)) = tokio::time::timeout(window, stream.read(&mut chunk)).await {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n]);
    }
    out
}

async fn send_byte(stream: &mut TcpStream, byte: u8) {
    stream.write_all(&[byte]).await.expect("write byte");
    stream.flush().await.expect("flush");
}

#[tokio::test]
async fn n_echoes_on_keypress_and_enter_quits() {
    let addr = spawn_runtime().await;
    let mut stream = login(addr).await;
    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, b"uit:\x1b[0m ").await; // More? prompt tail

    send_byte(&mut stream, b'n').await;
    let echoed = read_for(&mut stream, Duration::from_millis(300)).await;
    assert_eq!(echoed, b"n", "n must echo on the keypress itself");

    send_byte(&mut stream, b'\r').await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    // Probe P1 (ae_tierd_probes.txt:100-138): the CR echoes \r\n and
    // the exit tail follows directly — no Quit word, no BS-SP-BS.
    assert!(
        contains(&after, b"\r\n\x1b[0m\r\n\x1b[0m\r\n"),
        "Enter after held n quits straight into the exit tail (probe P1)"
    );
    assert!(
        !contains(&after, b"Quit"),
        "held-n + Enter must NOT echo the Quit word (probe P1)"
    );
    end_session(&mut stream).await;
}

#[tokio::test]
async fn q_acts_on_a_single_keypress_without_enter() {
    let addr = spawn_runtime().await;
    let mut stream = login(addr).await;
    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, b"uit:\x1b[0m ").await;

    send_byte(&mut stream, b'Q').await;
    let after = drain_until(&mut stream, b"mins. left): ").await;
    assert!(contains(&after, b"Quit\r\n"), "Q quits with no terminator");
    end_session(&mut stream).await;
}

#[tokio::test]
async fn flag_entry_echoes_each_typed_byte() {
    let addr = spawn_runtime().await;
    let mut stream = login(addr).await;
    write_line(&mut stream, b"F 1").await;
    drain_until(&mut stream, b"uit:\x1b[0m ").await;

    send_byte(&mut stream, b'F').await;
    drain_until(&mut stream, b"to flag:\x1b[0m ").await;
    for &b in b"TERMV48.LHA" {
        send_byte(&mut stream, b).await;
        let echoed = read_for(&mut stream, Duration::from_millis(300)).await;
        assert_eq!(echoed, vec![b], "each flag byte echoes as typed");
    }
    send_byte(&mut stream, b'\r').await;
    drain_until(&mut stream, b"uit:\x1b[0m ").await; // More? redrawn
    send_byte(&mut stream, b'Q').await;
    drain_until(&mut stream, b"mins. left): ").await;
    end_session(&mut stream).await;
}
```

(`F 1` must page: confirm the seeded dir 1 exceeds 29 lines — `seed::demo_file_catalogue` carries 27 Dir1 records, well past one page. If the More?-prompt drain sentinel differs byte-wise, take the exact tail from `wire.rs::MORE_PROMPT`.)

- [ ] **Step 2: Run**: `cd rust && cargo nextest run tierd_hotkey` — PASS. These three tests are the "fix actually fixes it" gate: they fail against the pre-D2b line-read pager.
- [ ] **Step 3: Commit**

```bash
git add rust/tests/tierd_hotkey_smoke.rs
git commit -m "Tests: char-mode client smoke — echo on keypress, act without Enter (slice D2b)"
```

### Task 2.7: D2b docs + mutants

**Files:**
- Modify: `COMMAND_PARITY.md` (:724), `designs/NEXTSCAN.md` (:51, :55), `SLICES.md` (F row), `SYSTEM.md`

- [ ] **Step 1:** COMMAND_PARITY.md:724 — replace the "Enter-required pager keys" COSMETIC row with: pager prompts are true hotkeys, parity restored; held-`n`+Enter and bare-LF behaviours pinned by `ae_tierd_probes.txt` (cite P1/P2); case-insensitive verbs recorded as inference (only `Q/Y/n/ns` cases captured).
- [ ] **Step 2:** NEXTSCAN.md:51 — rewrite the "Input mechanism — decision" paragraph: D2b landed; `TerminalEcho::Silent` removed; `Terminal::read_key` is the pager read. :55 — update the lone-`n` paragraph with the probe-pinned Enter rule.
- [ ] **Step 3:** SLICES.md — mark D2b done on the F row; SYSTEM.md — add `read_key` to the Terminal port description (and the diagram if it names port methods).
- [ ] **Step 4: Mutants**: `cd rust && cargo mutants --file src/app/menu_flow/file_list/mod.rs --file src/adapters/telnet_line.rs` — 0 missed or each survivor explained in the commit message.
- [ ] **Step 5: Commit**

```bash
git add COMMAND_PARITY.md designs/NEXTSCAN.md SLICES.md SYSTEM.md
git commit -m "Docs: D2b recorded — hotkey parity restored, probe corners pinned"
```

---

## Phase 3 — Slice D2f: the `[X]` flag marker

### Task 3.1: `FlaggedFiles` domain set

**Files:**
- Create: `rust/src/domain/files/flagged.rs`
- Modify: `rust/src/domain/files/mod.rs` (add `pub(crate) mod flagged;`)

- [ ] **Step 1: Write the failing test** (in `flagged.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flagging_is_case_insensitive_and_idempotent() {
        let mut flags = FlaggedFiles::default();
        assert!(flags.flag(FlaggedKey::new(2, 1, "termv48.lha")));
        assert!(!flags.flag(FlaggedKey::new(2, 1, "TERMV48.LHA")), "same file");
        assert!(flags.contains(&FlaggedKey::new(2, 1, "TermV48.LHA")));
        assert!(!flags.contains(&FlaggedKey::new(2, 2, "TERMV48.LHA")), "other area");
    }
}
```

- [ ] **Step 2: Run to verify failure**: `cd rust && cargo nextest run flagged` — FAIL (module missing).
- [ ] **Step 3: Implement**:

```rust
//! Session-scoped flagged files — slice D2f, the in-memory precursor
//! to slice D5's persisted `FlaggedFile` (`amiexpress/express.e:2757`
//! loadFlagged / `:2798` saveFlagged own persistence later).

use std::collections::BTreeSet;

/// Catalogue identity of a flaggable file. Names compare
/// case-insensitively (stored uppercase) — the DIR catalogue is
/// case-preserving but the legacy flag prompt is not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FlaggedKey {
    conference: u32,
    area: u32,
    name: String,
}

impl FlaggedKey {
    /// Builds a key; `name` is folded to uppercase.
    pub(crate) fn new(conference: u32, area: u32, name: &str) -> Self {
        Self {
            conference,
            area,
            name: name.to_ascii_uppercase(),
        }
    }
}

/// The session's flagged-file set. Slice D5 will persist it; until
/// then it lives and dies with the session.
#[derive(Debug, Default)]
pub(crate) struct FlaggedFiles {
    set: BTreeSet<FlaggedKey>,
}

impl FlaggedFiles {
    /// Flags `key`. Returns `true` when newly flagged — the repaint
    /// trigger; re-flagging is a no-op.
    pub(crate) fn flag(&mut self, key: FlaggedKey) -> bool {
        self.set.insert(key)
    }

    /// Whether `key` is flagged.
    pub(crate) fn contains(&self, key: &FlaggedKey) -> bool {
        self.set.contains(key)
    }
}
```

Also give `FlaggedKey` the accessor the flag-entry matcher (Task 3.4) compares against:

```rust
    /// The uppercase-folded file name.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
```

- [ ] **Step 4: Run**: `cd rust && cargo nextest run flagged` — PASS. **Step 5: Commit**

```bash
git add rust/src/domain/files/
git commit -m "Files: FlaggedFiles session set — the D5 persistence precursor (slice D2f)"
```

### Task 3.2: Session plumbing

**Files:**
- Modify: `rust/src/domain/session/mod.rs` (the `Session` struct), `rust/src/domain/session/typed.rs` (`MenuSession` at :245)

- [ ] **Step 1: Write the failing test** in `typed.rs`'s test module (or `session/tests.rs`, matching where `MenuSession` accessors are tested today):

```rust
    #[test]
    fn menu_session_exposes_the_flag_set() {
        // Build a MenuSession the way the neighbouring tests do, then:
        let mut session = /* existing MenuSession test constructor */;
        let key = crate::domain::files::flagged::FlaggedKey::new(2, 1, "TERMV48.LHA");
        assert!(session.flagged_files_mut().flag(key.clone()));
        assert!(session.flagged_files_mut().contains(&key));
    }
```

(Use the construction helper the surrounding tests use — `typed.rs` tests build sessions for every accessor; mirror the nearest one.)

- [ ] **Step 2: Run to verify failure**, **Step 3: Implement** — `Session` gains a `flagged_files: FlaggedFiles` field, initialised `FlaggedFiles::default()` at every `Session` construction site (the compiler lists them), plus:

```rust
    /// The session's flagged files (slice D2f; D5 persists).
    pub(crate) fn flagged_files_mut(&mut self) -> &mut FlaggedFiles {
        &mut self.flagged_files
    }
```

and the `MenuSession` passthrough in `typed.rs`:

```rust
    /// The session's flagged-file set — the F/R pager verbs mutate it.
    pub(crate) fn flagged_files_mut(&mut self) -> &mut FlaggedFiles {
        self.session.flagged_files_mut()
    }
```

- [ ] **Step 4: Run full suite, Step 5: Commit**

```bash
git add rust/src/domain/session/
git commit -m "Session: flagged-file set rides the session (slice D2f)"
```

### Task 3.3: Marker slot in the rendered rows

**Files:**
- Modify: `rust/src/app/menu_flow/file_list/wire.rs` (`assemble_dir_lines` :210-239, `framed_row` :99-115, new types + marker fn; all row-shape tests)

This is the big re-pin: every aligned row gains 4 columns. The renderer keeps `dir_row.rs` as the untouched legacy-format authority; the marker slot is spliced in at the NextScan layer.

- [ ] **Step 1: Write the failing tests** (in `wire.rs` tests):

```rust
    use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};

    #[test]
    fn aligned_rows_carry_the_marker_slot() {
        // Design 2026-06-12 §5: 4-column slot between the 13-char name
        // field and the check byte; [X] when flagged, spaces otherwise.
        // Deliberate NextExpress departure (COMMAND_PARITY.md row).
        let files = vec![seeded(
            "ANSIPACK.LHA",
            234_567,
            Some(b'P'),
            time::macros::datetime!(2026-01-15 12:00 UTC),
            "Collection of 40 ANSI screens from the\nMirage art crew, January release.",
        )];
        let mut flagged = FlaggedFiles::default();
        flagged.flag(FlaggedKey::new(2, 1, "ANSIPACK.LHA"));

        let plain = assemble_dir_lines(&files, &FlaggedFiles::default());
        let marked = assemble_dir_lines(&files, &flagged);
        // Unflagged: 4 spaces in the slot; description col 33 -> 37,
        // continuations indented 37.
        assert_eq!(
            marked[5].bytes,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m[X] P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the".to_vec(),
        );
        assert_eq!(
            plain[5].bytes,
            b"\x1b[0m\x1b[36mANSIPACK.LHA \x1b[34m    P\x1b[32m 234567  \x1b[33m01-15-26\x1b[0m  Collection of 40 ANSI screens from the".to_vec(),
        );
        assert_eq!(
            marked[6].bytes,
            plain_line(b"                                     Mirage art crew, January release.").bytes,
        );
        // The first row of each file carries its listing identity.
        let listed = marked[5].listed.as_ref().expect("first row is listed");
        assert_eq!(listed.number, Some(1));
        assert!(flagged.contains(&listed.key));
    }

    #[test]
    fn overlong_names_append_the_marker_when_flagged() {
        let files = vec![seeded(
            "THIRTEENCH.LZ",
            66_666,
            None,
            time::macros::datetime!(2026-05-20 12:00 UTC),
            "Exactly thirteen character filename",
        )];
        let mut flagged = FlaggedFiles::default();
        flagged.flag(FlaggedKey::new(2, 1, "THIRTEENCH.LZ"));
        let lines = assemble_dir_lines(&files, &flagged);
        assert_eq!(
            lines[0].bytes,
            plain_line(b"THIRTEENCH.LZ   66666  05-20-26  Exactly thirteen character filename [X]").bytes,
        );
    }
```

NOTE the index/colour-boundary specifics above encode the design's geometry decision and MUST be reconciled with the actual splice rule in Step 3: slot bytes sit between name field and check byte, so `framed_row`'s blue span starts at the slot (`[X] ` or four spaces) + check byte — i.e. cyan cols 0-12, blue cols 13-17 (slot+check), green size, yellow date. If you instead keep the slot cyan, adjust BOTH test and implementation together and note the choice; the marker itself must introduce no new SGR (design §5).

- [ ] **Step 2: Run to verify failure** (signature change makes this a compile failure first — fine).
- [ ] **Step 3: Implement.** In `wire.rs`:

```rust
use crate::domain::files::flagged::{FlaggedFiles, FlaggedKey};

/// One assembled listing line plus, on a file's first row, its
/// catalogue identity — the pager records these for flag matching
/// and in-place repaint (slice D2f).
pub(super) struct ScanLine {
    pub(super) bytes: Vec<u8>,
    pub(super) listed: Option<ListedRow>,
}

/// A listed file: its flag key and its `[ File #N ]` number (framed
/// rows only — plain rows consume no number, `ae_tierd_aquascan3.txt`
/// S7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ListedRow {
    pub(super) key: FlaggedKey,
    pub(super) number: Option<u32>,
    /// Whether the row carries the aligned marker slot (name < 13
    /// chars); over-long rows append ` [X]` instead, and repaint
    /// targets a different column.
    pub(super) aligned: bool,
}

/// The 4-column marker slot (`[X] `/spaces) spliced between the name
/// field and the check byte — a deliberate NextExpress departure
/// (design 2026-06-12 §5; COMMAND_PARITY.md row). Over-long names
/// (≥ 13 chars, already unaligned in the legacy format) append
/// ` [X]` when flagged instead.
const MARKER_FLAGGED: &[u8] = b"[X] ";
const MARKER_EMPTY: &[u8] = b"    ";
```

`plain_line` and the helpers now return `ScanLine` (wrap the existing bytes; `listed: None`). `assemble_dir_lines(files: &[File], flagged: &FlaggedFiles) -> Vec<ScanLine>`: for each file compute `let key = FlaggedKey::new(file.conference(), file.area(), file.name());` and `let is_flagged = flagged.contains(&key);` — splice the slot into the first row when `file.name().len() < 13` (insert `MARKER_*` at byte offset 13 of the dir_row first row, BEFORE framing), append `b" [X]"` to over-long flagged rows, indent continuations 37 (i.e. `4 + 33`) for aligned rows and keep 33 for over-long rows. `framed_row` shifts its colour-span offsets +4 (name cols `..13`, slot+check `13..18` blue, size `18..27` green, date `27..35` yellow, reset, rest). The footer/separator/header lines wrap with `listed: None`. (If `File` lacks `conference()`/`area()` accessors, add them in `rust/src/domain/files/file.rs` with doc comments — the fields exist per the D1 struct.)

In `file_list/mod.rs`: `stream_dir_body` iterates `wire::assemble_dir_lines(files, flagged)` — thread `session` (or `&mut FlaggedFiles` + a `&FlaggedFiles` view) from `handle_file_list` through `run_span`/`stream_dir_body`/`emit_scan_line` (change `emit_scan_line` to take `ScanLine`; `ScanState.page` becomes `Vec<PageLine>` where `struct PageLine { bytes: Vec<u8>, listed: Option<wire::ListedRow> }`; the `?` redraw uses `.bytes`). `ScanState` gains `listed: Vec<wire::ListedRow>` appended as lines stream — the scan-wide registry for F/R matching.

- [ ] **Step 4: Re-pin the shifted expectations** — every aligned-row literal in `wire.rs` tests, `file_list/mod.rs` tests and `tierd_file_list_smoke.rs` gains the 4-space slot (mechanical: insert 4 spaces after the name field, shift continuation indents 33→37). The dir_row.rs tests are untouched (legacy layer).
- [ ] **Step 5: Run full suite + build** — green, no warnings. **Step 6: Commit**

```bash
git add rust/src/app/menu_flow/file_list/ rust/src/domain/files/ rust/tests/tierd_file_list_smoke.rs
git commit -m "Menu: rows grow the [X] marker slot; lister tracks listed identities (slice D2f)"
```

### Task 3.4: F/R entries mutate the flag set; repaint in place

**Files:**
- Modify: `rust/src/app/menu_flow/file_list/mod.rs` (the F/R arm from Task 2.4; new `apply_flags` + `repaint_flagged_rows`; tests)
- Modify: `rust/src/app/menu_flow/file_list/wire.rs` (promote the test-only width helper: `pub(super) fn visible_columns(bytes: &[u8]) -> usize` — move the existing test `visible_width` logic, byte-based is correct here because rendered rows are ASCII outside SGR runs except the separator art, which is never a repaint target)

- [ ] **Step 1: Write the failing tests** (file_list/mod.rs tests; key-scripted per Task 2.3):

```rust
    #[tokio::test]
    async fn flagging_a_visible_row_paints_the_marker_in_place() {
        // Design 2026-06-12 §5: F TERMV48.LHA while its row is on the
        // current page -> cursor up to the row, [X] into the slot,
        // cursor back, More? redrawn. Keys: F, T,E,R,M,V,4,8,.,L,H,A,
        // Enter, then Q.
        // Assert the output contains, after the 79-space overprint:
        //   \r ESC[{k}A ESC[14G [X] \r ESC[{k}B
        // with k = lines between TERMV48's row and the prompt line in
        // the driven listing, followed by MORE_PROMPT.
    }

    #[tokio::test]
    async fn r_flags_by_listing_number() {
        // R then "1" + Enter flags [ File #1 ]; assert the flag set
        // contains its key afterwards and the repaint targeted #1's row.
    }

    #[tokio::test]
    async fn unmatched_flag_entries_change_nothing() {
        // F NOSUCH.LHA: no repaint bytes, no set mutation, More?
        // redrawn — the captured silent-ignore (the accidental
        // capture fed junk silently, ae_tierd_aquascan_accidental.txt).
    }

    #[tokio::test]
    async fn repaint_is_suppressed_when_ansi_is_off() {
        // Same drive as the first test but terminal.set_ansi_colour(false)
        // (or a fake reporting ansi_colour()=false): flag succeeds (set
        // mutated), but no ESC[…A/G/B bytes are emitted.
    }
```

Write them fully against the existing scaffolding; compute `k` from the test's own listing layout rather than hard-coding (count emitted lines after the target row, +1 for the prompt line being one below the last body line).

- [ ] **Step 2: Run to verify failure.**
- [ ] **Step 3: Implement.** The F/R arm becomes:

```rust
                    self.terminal.write(&more_overprint_clear()).await?;
                    self.terminal.write(prompt).await?;
                    let Some(entry) = self.read_flag_entry().await? else {
                        return Ok(ScanFlow::Quit);
                    };
                    let newly = apply_flags(&entry, !by_name, state, session.flagged_files_mut());
                    self.terminal.write(&flag_overprint_clear()).await?;
                    self.repaint_flagged_rows(state, &newly).await?;
                    self.terminal.write(wire::MORE_PROMPT).await?;
```

with:

```rust
/// Applies a flag-prompt entry (whitespace-separated names for F,
/// `[ File #N ]` numbers for R) against the scan's listed registry.
/// Unmatched tokens are silently ignored (the door accepts junk
/// silently — accidental capture). Returns the NEWLY flagged keys,
/// the repaint set.
fn apply_flags(
    entry: &str,
    by_number: bool,
    state: &ScanState,
    flagged: &mut FlaggedFiles,
) -> Vec<FlaggedKey> {
    let mut newly = Vec::new();
    for token in entry.split_whitespace() {
        let matched: Option<&wire::ListedRow> = if by_number {
            token
                .parse::<u32>()
                .ok()
                .and_then(|n| state.listed.iter().find(|row| row.number == Some(n)))
        } else {
            let wanted = token.to_ascii_uppercase();
            state.listed.iter().find(|row| row.key.name() == wanted)
        };
        if let Some(row) = matched {
            if flagged.flag(row.key.clone()) {
                newly.push(row.key.clone());
            }
        }
    }
    newly
}
```

(`FlaggedKey::name()` is the accessor added in Task 3.1; names are stored uppercase, so the comparison is case-insensitive.) NOTE: `scan_more_prompt`'s signature gains the session — `scan_more_prompt(&mut self, state: &mut ScanState, session: &mut MenuSession)` — threaded from `handle_file_list` through `run_span`/`post_end_pause`/`stream_dir_body`/`emit_scan_line` exactly as Task 3.3 threads the flag set for rendering.

```rust
    /// Paints `[X]` into the marker slot of any newly flagged row
    /// still on the current page: `\r`, cursor up k, column 14
    /// (1-based; the slot starts at visible column 14 on aligned
    /// rows), the marker, then back down to the prompt line. Rows
    /// off-page show their marker at next render. Suppressed for
    /// non-ANSI terminals — cursor CSI would garble them
    /// (ColourTerminal strips SGR only).
    async fn repaint_flagged_rows(
        &mut self,
        state: &ScanState,
        newly: &[FlaggedKey],
    ) -> Result<(), T::Error> {
        if newly.is_empty() || !self.terminal.ansi_colour() {
            return Ok(());
        }
        for (index, line) in state.page.iter().enumerate() {
            let Some(listed) = &line.listed else { continue };
            if !newly.contains(&listed.key) {
                continue;
            }
            let up = state.page.len() - index;
            // Aligned rows: slot at visible col 14 (1-based). Over-long
            // rows: append after the row's last visible column.
            let column_cmd = if listed.aligned {
                "\x1b[14G[X]".to_string()
            } else {
                format!("\x1b[{}G [X]", wire::visible_columns(&line.bytes) + 1)
            };
            let seq = format!("\r\x1b[{up}A{column_cmd}\r\x1b[{up}B");
            self.terminal.write(seq.as_bytes()).await?;
        }
        Ok(())
    }
```

(`listed.aligned` is set by the renderer in Task 3.3 — true when `name.len() < 13` — so the repaint never re-derives geometry from bytes.)

- [ ] **Step 4: Add the smoke** — in `tierd_hotkey_smoke.rs`, extend `flag_entry_echoes_each_typed_byte`: after the Enter, assert the drained bytes match `\r ESC[{k}A ESC[14G [X] \r ESC[{k}B` for TERMV48's on-page position before the More? redraw, and after a fresh `F 1` re-list, assert the TERMV48 row contains `[X] P`.
- [ ] **Step 5: Run full suite + build** — green. **Step 6: Commit**

```bash
git add rust/src/app/menu_flow/file_list/ rust/tests/tierd_hotkey_smoke.rs
git commit -m "Menu: F/R flag for real — [X] painted onto visible rows in place (slice D2f)"
```

### Task 3.5: D2f docs + mutants + SYSTEM.md

**Files:**
- Modify: `COMMAND_PARITY.md`, `designs/NEXTSCAN.md` (§10 / slice list), `SLICES.md`, `SYSTEM.md`, `slices/cmds-files-list.md`

- [ ] **Step 1:** COMMAND_PARITY.md — add the marker-slot row (deliberate departure: legacy rows have no marker; cite design §5) and the repaint behaviour (NextExpress-only, ANSI-gated).
- [ ] **Step 2:** NEXTSCAN.md + `slices/cmds-files-list.md` — record D2f as landed; add the future-slice entries per design §8: **D5** (FlaggedFile persistence + logon `** Flagged File(s) Exist **` + BEL `express.e:2791-2794` + logoff `checkFlagged` `express.e:12667-12673` + `** AutoSaving File Flags **` `express.e:2803` + download integration), **A alter-flags** verb, and the fresh capture session for AquaScan's `A`/`D` verbs.
- [ ] **Step 3:** SYSTEM.md — FlaggedFiles in the domain section; diagram updated if it lists domain modules.
- [ ] **Step 4: Mutants over the slice**: `cd rust && cargo mutants --file src/domain/files/flagged.rs --file src/app/menu_flow/file_list/wire.rs --file src/app/menu_flow/file_list/mod.rs` — 0 missed or survivors explained.
- [ ] **Step 5: Commit**

```bash
git add COMMAND_PARITY.md designs/NEXTSCAN.md SLICES.md SYSTEM.md slices/cmds-files-list.md
git commit -m "Docs: D2f recorded — marker slot, repaint, D5 future surfaces scheduled"
```

---

## Phase 4 — Acceptance

### Task 4.1: AGENTS.md gains the type-at-it checklist item

**Files:**
- Modify: `AGENTS.md` (the `## Before Committing` list)

- [ ] **Step 1:** Append item 6:

```markdown
6. For any slice that changes user-facing interaction: boot the server
   (`cargo run -- nextexpress.toml` or the built binary), connect with a
   plain UTF-8 terminal client (`telnet 127.0.0.1 2323`), and exercise
   the new surface BY TYPING — checking per-keystroke echo, terminators,
   and rendering. Scripted byte-equality cannot observe these
   (RCA 2026-06-11; designs/2026-06-12-utf8-hotkeys-flagmark-design.md §6.3).
```

(Verify the port/config invocation against `rust/src/bin` / `nextexpress.toml` before writing it down.)

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "Docs: Before-Committing item 6 — a human types at user-facing slices"
```

### Task 4.2: Full verification pass

- [ ] **Step 1:** `cd rust && cargo nextest run && cargo build && cargo test --doc` — all green, no warnings.
- [ ] **Step 2:** `cd rust && cargo mutants` (full run; this is the slice-complete gate AGENTS.md requires) — triage every survivor: kill with a test or explain in the final commit.
- [ ] **Step 3:** Boot the server and drive a raw char-at-a-time session (the executor does this; e.g. a 10-line tokio or Python script sending single bytes with 200 ms gaps through login → `F 1` → `n`, Enter → `F 1` → `F`, type a name, Enter → `Q` → `G`), confirming by eye: per-key echo at every prompt, wave art renders as `_¸,ø*¤°¬°¤*ø,¸_`, `[X]` appears in place.
- [ ] **Step 4:** Report to Paul for the human type-at-it gate (acceptance criterion 5) — the work is not "done" until he has typed at it.
