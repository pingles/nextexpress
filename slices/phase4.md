# Phase 4 — Sysop console & node controls

Lets the sysop log on locally, reserve nodes, suspend / resume / shut down
nodes, and kick a session off another node.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 22 — Sysop direct logon
- **In Scope**
  - `session.allium:SysopDirectLogon` — F1-equivalent local key shortcut on the BBS console creates a session at `state = onboarded` for `sysop_user()` (`slot_number = 1`), `channel = sysop_console`, skipping identification/auth.
- **Out of Scope**
  - F2-style "local logon" (Slice 23).
  - "instantLogon" sysop key combo (`session.allium` open question).

## Slice 23 — Local logon + relogon
- **In Scope**
  - `LogonChannel::local` — F2 path: still goes through identification/auth, but `online_baud = 0` and `is_remote = false`.
  - `session.allium:RelogonRequested` — session ends with `relogon`; `ReleaseNode` flips node back to `connecting` instead of `idle`.
- **Out of Scope**
  - Sysop "switch user" UX wrapping relogon.

## Slice 24 — Node reservation
- **In Scope**
  - Adds `Node.reserved_for: Option<UserId>`, the `reserved` status and the `idle -> reserved -> idle` / `reserved -> connecting` transitions plus the `ReservedHasUser` invariant.
  - `session.allium:ReserveNodeForUser` and `ClearNodeReservation` rules.
  - `AcceptConnection` rejects with `reserved_for_other` when the connecting user is not the reserved one.
- **Out of Scope**
  - The "page reserved-for-X user" out-of-band notification.

## Slice 25 — Node suspend / resume / shutdown
- **In Scope**
  - Adds the `suspended` and `shutting_down` statuses and the `idle -> suspended -> idle` and `idle -> shutting_down` transitions.
  - `session.allium:SuspendNode`, `ResumeNode`, `InitiateShutdown` rules.
  - Cooperative shutdown — active sessions log off on their own clock per the rule's `@guidance`.
- **Out of Scope**
  - OS-level signal handling for graceful daemon stop (config concern).

## Slice 26 — Sysop kick
- **In Scope**
  - `session.allium:SysopKick` — sysop console command kicks a session on another node; `logoff_reason = sysop_kicked`.
- **Out of Scope**
  - Inter-node messaging (`OLM`); kick is a direct sysop action only.
