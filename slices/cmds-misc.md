# Tier J — Lower-priority / niche commands

These commands exist in the legacy dispatch table but rarely appear
in the default menus, depend on subsystems (QWK packets, voting
infrastructure) that the spec deliberately scopes out, and would
deliver less value per slice than anything in Tiers A–I.

See [SLICES.md](../SLICES.md). The Tier J slices should be reviewed
before they're worked — if a sysop community signal says one of
these matters, promote it into an earlier tier rather than
implement-by-default.

## Slice J1 — `ZOOM` (mail gather)

- **In Scope**
  - Parser: `MenuCommand::Zoom`.
  - Picks the user's `zoomType` (`User.zoom_type`) — currently the
    spec gives us two values: `0 = qwk`, `1 = ascii`.
  - Implements only the `ascii` branch (legacy `asciiZoom` at
    `amiexpress/express.e:26234`); QWK is in
    [future.md](future.md) and remains there.
- **Out of Scope**
  - QWK packet generation — `messaging.allium` deliberately scopes
    it out.

## Slice J2 — `VO` (voting booth)

- **In Scope**
  - Parser: `MenuCommand::Vote`.
  - The legacy distinguishes `vote()` vs `voteMenu()` by
    `ACS_MODIFY_VOTE` (`amiexpress/express.e:25701-25709`).
  - Both call into a tiny voting-question / answer model that we
    introduce here under a new spec file (`voting.allium`) — to be
    drafted as part of this slice.
- **Out of Scope**
  - Anonymous / weighted vote counting — out of scope for the
    legacy too.

## Slice J-wire — Tier J wire-and-smoke

- **In Scope**
  - Smoke each command in turn against the binary.
- **Out of Scope**
  - Wide-scale fixture vote / zoom corpora — single fixture suffices.
