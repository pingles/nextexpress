# Phase 4 — Conferences (read)

Conferences and message bases as data, with auto-rejoin on logon, the `J`
command, the conference scan walk, and per-conference menus / bulletins /
name-type promotion.

See [SLICES.md](../SLICES.md) for the schema-growth principle, progress
table and asset inventory.

## Slice 27 — Conference + MessageBase entities
- **In Scope**
  - `core.allium:Conference`, `MessageBase` data only, with `AtLeastOneMessageBase` invariant.
  - `MessageBaseRef` value type and `msgbase_ref_for(msgbase)` helper.
- **Out of Scope**
  - File areas (Slice 50).
  - "Custom" / bridged bases (deferred per spec).

## Slice 28 — Conference loader from disk
- **In Scope**
  - Read a conference layout that mirrors `defaultbbs/Conf01/{path,paths,NDirs,Conf.DB}` — but in a Rust-friendly format (TOML), per `AGENTS.md`.
  - Reference the legacy seed files in test fixtures so the loader's layout assumptions are explicit.
- **Out of Scope**
  - Editing conferences at runtime (Slice 36).

## Slice 29 — `ConferenceMembership` + access checks
- **In Scope**
  - Adds `User.memberships`, `User.last_joined_conference`, `User.last_joined_msgbase` (first read here).
  - `core.allium:ConferenceMembership` entity with `granted` and message-counter fields actually consumed by Phase 4; ratio fields and per-conf byte tallies land with their slices (61).
  - `has_membership(user, conference)` and `first_accessible_conference(user)` black-box functions.
- **Out of Scope**
  - Per-conference accounting on transfers (Slice 61).

## Slice 30 — `JoinConference` (auto-rejoin on logon)
- **In Scope**
  - `conferences.allium:JoinConference` for `auto_rejoin` — uses `User.last_joined_conference` / `last_joined_msgbase`, falls back to `first_accessible_conference`.
  - `ConferenceVisit` entity created; `LeaveConferenceOnSwitch` closes prior visits.
  - `SessionsHaveAtMostOneOpenVisit` and `VisitedMsgBaseBelongsToVisitedConference` invariants.
  - When no conferences are accessible, session ends with `no_conference_access`.
- **Out of Scope**
  - Bulletins (Slice 31) and mail scan triggers (Slice 41).

## Slice 31 — Conference / node bulletins + per-conference menu
- **In Scope**
  - `conferences.allium:ShowConferenceBulletin` after a join, suppressed under `quick_logon` or during a multi-conf scan.
  - Per-conference menu resolution: prefer `Conf<n>/menu.txt` over the hard-coded `Conf02/Menu.txt` used pre-Phase-5; fall back to a system-wide menu.
  - Use `defaultbbs/Conf01/menu.txt` as the low-access tier sample fixture.
- **Out of Scope**
  - User-flag-driven bulletin suppression (`show_one_time_messages`, `screen_clear_after_message`).
  - Access-level-aware `Menu<N>.txt` walk — already pulled forward to Slice 21 (`amiexpress/express.e:6246` findSecurityScreen).

## Slice 32 — Explicit `J` (join conference) command
- **In Scope**
  - User typing `J` from the menu fires `JoinConferenceRequested(reason=explicit_join)`.
  - JOIN / JOINED / JOINCONF screens displayed at the right points (`amiexpress/express.e:25143`).
- **Out of Scope**
  - Conference scan walk (Slice 33).

## Slice 33 — `ConferenceScan` (CS command)
- **In Scope**
  - `conferences.allium:StartConferenceScan`, `StepConferenceScan`, `FinishConferenceScan`.
  - Re-join the user's last conference at the end of the scan.
- **Out of Scope**
  - Mail scan integration — Slice 41 ties them together.

## Slice 34 — `JoinedConferenceForNameType`
- **In Scope**
  - Adds `Session.display_name_type` field (first read here).
  - `conferences.allium:JoinedConferenceForNameType` flips `session.display_name_type` to the conference's `accepted_name_type`.
  - Real-name / internet-name screens displayed when promoted (`SCREEN_REALNAMES` / `SCREEN_INTERNETNAMES`, `amiexpress/express.e:28169`).
- **Out of Scope**
  - Editing the user's `real_name` / `internet_name` (Slice 66).
