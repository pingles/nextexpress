# Tier H — Sysop console (`F6`-class) commands

Numeric and short-token commands that the legacy reserves for sysops
logged on at the console (or via remote shell). They're an
operational pillar but they ship after Tiers A–G because each one
duplicates a job that file-based config or the host shell can do
externally.

See [SLICES.md](../SLICES.md). The legacy command `CM` (conference
maintenance) is **Skipped** for the same reason as the original
Phase 5 — see the "Skipped slices" section of SLICES.md.

## Slice H1a — `1` (account editor: list, search, read-only summary)

- **In Scope**
  - Parser: `MenuCommand::AccountEditing`.
  - Sysop-only `F6` account editor surface — list / search by slot
    or by name / select a user, display the selected user's record
    in the legacy `[34m[…][32mField[33m:[0m value` shape.
  - Mirrors `internalCommand1` → `editAccounts(FALSE)`
    (`amiexpress/express.e:24453-24459`), read-only half only.
- **Why split**: a sysop very often runs `1` just to look someone
  up. Read-only is the most-used branch and lands an immediately
  useful debugging surface before any field-mutation code exists.

## Slice H1b — `1` (account editor: field edits)

- **In Scope**
  - The edit-and-write-back-via-`UserRepository` loop for each
    editable field — same pattern Tier F's `W` slices establish,
    extended to the sysop-only fields (security level,
    conference access string, force-password-reset, etc.).
- **Out of Scope**
  - Bulk import / CSV — a CLI-side wizard (Future).

## Slice H2 — `2` (caller-log viewer)

- **In Scope**
  - Parser: `MenuCommand::CallerLog(NumberArg)`.
  - Reads `<bbs-loc>/Node<n>/Callerslog`, paginates per the legacy
    (`amiexpress/express.e:24461-24508`).
- **Out of Scope**
  - Centralised log aggregation — out of scope; sysops can tail the
    per-node logs externally.

## Slice H3 — `UP` (node uptime)

- **In Scope**
  - Parser: `MenuCommand::NodeUptime`.
  - Emits `Node <n> was started at <ts>` per
    `internalCommandUP` (`amiexpress/express.e:25667-25673`).
- **Out of Scope**
  - Cluster-wide uptime — only this node.
- **Why it's high in this tier**: trivial wiring, useful on-call
  command.

## Slice H4 — `NM` (node monitor)

- **In Scope**
  - Parser: `MenuCommand::NodeMonitor`.
  - Interactive change-node sub-prompt
    (`amiexpress/express.e:25281-25370`) — for now, lists nodes and
    accepts a `Q`uit response, without the sysop-issues-action
    branch (that's the next slice).
- **Out of Scope**
  - Sending OLMs / kicks from within `NM`.

## Slice H5 — `NM` actions: kick / OLM from sysop console

- **In Scope**
  - Wires `NM` to the existing `SysopKick` and `OLM` rules from
    Tiers G and E.
- **Out of Scope**
  - Real-time stats overlays.

## Slice H6 — `3` / `4` / `5` (file editing primitives)

- **In Scope**
  - `MenuCommand::EditDirFile(params)` →
    `editDirFile(params)` (legacy: `amiexpress/express.e:24511`).
  - `MenuCommand::EditAnyFile(params)` →
    `editAnyFile(params)` (`:24517`).
  - `MenuCommand::DirAnywhere(params)` →
    `myDirAnyWhere(params)` (`:24523`).
  - Backed by a configured `$EDITOR` (TOML setting) — we don't
    re-implement the legacy emacs surface.
- **Out of Scope**
  - The legacy Amiga-emacs UI itself — using `$EDITOR` is the
    intentional divergence per `AGENTS.md`.

## Slice H7 — `0` (remote shell)

- **In Scope**
  - Parser: `MenuCommand::RemoteShell`.
  - Spawns a sub-shell (legacy `remoteShell` at
    `amiexpress/express.e:24450`) and proxies bytes through the
    telnet stream.
  - Honours the `cmds.remotePass` two-factor gate
    (`amiexpress/express.e:24434-24449`).
- **Out of Scope**
  - PTY allocation across platforms beyond Unix — Windows defers
    to MSYS / WSL.

## Slice H-wire — Tier H wire-and-smoke

- **In Scope**
  - As sysop, run each command end-to-end against the binary.
  - Assert the smoke client never receives the password as cleartext
    on the `0`-command path (Adapter contract rule 1).
- **Out of Scope**
  - Multi-sysop concurrency — single-sysop assumption holds for the
    console.
