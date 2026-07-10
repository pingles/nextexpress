# artifact-conventions

Where each stage writes, in what shape, and the literal-authoring rules an
implementer MUST obey. Load this when producing any stage output. Paths are
repo-relative to the run's worktree root.

## Part 1 — stage → outputs

| Stage | Writes | Shape / convention |
|---|---|---|
| 1 Assess & plan | `slices/<family>.md` (In/Out-of-scope entry) + `SLICES.md` (roadmap row) | Append a `## Slice <id> — <title> — **<status>**` block to the family doc, mirroring the existing D-slice blocks in `slices/cmds-files-list.md`: name the command, its dispatch line (`express.e:28285`), In-scope / Out-of-scope bullets, and the declared **track** (six-stage vs refactor). Add/patch the matching roadmap row in `SLICES.md`. Name any pre-refactor here. |
| 2 Capture truth | `comparison/harness/<cmd>.py` + `comparison/transcripts/<cmd>.txt` + `comparison/evidence-<slice>/<note>.md` | Driver script under `comparison/harness/` (see the existing `rust_*` / `diff_*` scripts). Raw transcript under `comparison/transcripts/`. A human-readable experience note under `comparison/evidence-<slice>/` in the `live-observations.md` shape: capture date + fixture description, then per-block **RENDER + Python-repr byte forms**, with `express.e:N` cross-refs explaining the byte offsets. **Edit the Stage-1 slice doc to reference these captures** (transcript filename + evidence note) so command→experience is recorded, not folklore. |
| 3 Design | `designs/<YYYY-MM-DD>-<cmd>-design.md` | Date-prefixed, matching `designs/2026-07-03-n-newfiles-scan-design.md`. Contains: what changes, how it conforms to `specs/*.allium`, how it drives the Stage-2 captured behaviour, the **grammar table** (every input form → capture ref → intended handling, §10.4), any door/source A/B decision (§10.3), and the implementation plan. |
| 4 Build | `rust/` code + tests + `COMMAND_PARITY.md` rows + `SYSTEM.md` | Tests follow the sibling-`tests.rs` convention (§10.4): large/test-dominated modules get a sibling `tests.rs`, small ones stay inline — never `foo_test.rs` / `#[path]`. Append `COMMAND_PARITY.md` rows (incl. PLAUSIBLE rows for uncaptured edges) and update `SYSTEM.md` — **re-audit existing claims, do not just append** (§10.4). |
| 5 Compare | `comparison/evidence-<slice>/comparison-<YYYY-MM-DD>.md` | The synthesized double-blind divergence report: scenario set, both tester logs (NextExpress/telnet vs live FS-UAE/telnet), per-step input→observed, and the cross-marked divergence list. |
| — Run-state | `.command-slice/run-state.json` (worktree-local; add to `.git/info/exclude` so it is never staged) | Stage, scenario index, allocated ports, container name, server PID — updated at each stage boundary for resume (§10.5). Never committed or merged. |

Shared high-contention docs (`COMMAND_PARITY.md`, `SYSTEM.md`, `SLICES.md`,
`AGENTS.md`) are merge-prone: append at stable anchors and re-audit after rebase
(§10.9). Guard tests assert over **full, unfiltered** doc content.

## Part 2 — literal-authoring rules

Every rule here is a §10 invariant paid for by a prior reversal. See
`hardening.md` for the evidence trail.

### String provenance (§10.4)
Every user-facing wire literal carries either an `express.e:N` source comment or
a labelled deliberate-departure note **plus** a `COMMAND_PARITY.md` row. Review
grep-rejects unprovenanced wire strings. Binding letters cite their
`express.e:28285` dispatch line; menu-asset rows are diffed **verbatim** (no
token pre-filter).

### Encoding re-encode (§10.7)
Every captured byte ≥ 0x80 is recorded with **both** its Latin-1 byte and its
target UTF-8 code point. Test literals use the **`&str` code-point form** (e.g.
`\u{a9}`, `©`), never the raw byte. Emit **one `COMMAND_PARITY.md`
encoding-departure row per high-bit surface**. Naive verbatim paste = mojibake
before the e2e UTF-8 gate exists.

### Volatile vs stable field tagging (§10.6)
Tag every captured field stable-const or volatile-runtime.
- **Stable-const** (glyphs, prompts, dash geometry): byte-pin the literal.
- **Volatile-runtime** (dates, times, node/conf numbers, last-call-derived
  defaults): assert **format/derivation** only (e.g. `== user.last_call()`-derived
  default, `mm-dd-yy` shape). Never byte-pin the captured value — `06-25-26`
  drifts by capture day.

### No self-referential pins (§10.4)
Expected literals in a test are **independent bytes**, never derived from the
same const or `\`-continuation idiom they assert against (the D9 vacuous pin).

### PLAUSIBLE-row quarantine for uncaptured edges
An edge that is structurally uncapturable (timeout, door-pager consumption,
two-node block) is resolved from `express.e` control-flow, tagged
**extrapolated-from-source**, and recorded as a **PLAUSIBLE** `COMMAND_PARITY.md`
row — quarantined from the byte-pinned MATCH rows until a live capture confirms
it. Never guess it from partial bytes.

### Failure paths (§10.4)
Any handler touching a port/store gets a failing-adapter test proving a
*modelled error* with no partial commit. Review grep-rejects
`unwrap/expect/panic!` on port results.

## Tiny examples

**Illustrative shapes only** — substitute the slice's real captured literals and
`express.e` line numbers; do not copy these values.

Example `COMMAND_PARITY.md` encoding-departure row (a high-bit surface):

```
| `N` help banner (© glyph) | Emits `Copyright \u{a9} 1994` (UTF-8 U+00A9) | Latin-1 byte `0xa9` on the wire | ENCODING-DEPARTURE — re-encoded ≥0x80 to code point per §10.7 |
```

Example test literal (stable-const byte-pin + provenance comment; code-point form
for the high-bit glyph):

```rust
// express.e:19450 — NextScan listing banner; © re-encoded 0xa9 -> U+00A9 (§10.7)
assert_eq!(line, "--[ NextScan by NextExpress ]---[ 'f ?' for options ]--");
// volatile: assert derivation, not the captured 06-25-26 literal (§10.6)
assert_eq!(default_date, user.last_call().format(&mmddyy)?);
```
