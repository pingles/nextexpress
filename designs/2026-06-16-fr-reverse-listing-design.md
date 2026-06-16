# Slice D3 — `FR` (reverse file listing) — design

Tier D, slice D3 (`slices/cmds-files-list.md` §"Slice D3"). Ships the
`FR` command: the `NextScan`/AquaScan file lister in reverse
chronological order. Reuses the D2 `F` code path with one new
`reverse` seam.

## Parity authority (decided 2026-06-16)

Tier D file commands target the **AquaScan door (board-as-shipped)**,
not the shadowed `internalCommandF`/`FR`
(`amiexpress/express.e:24877/24883` → `displayFileList`,
`processCommand` dispatches the door icons first, `:28229-28256`). Where
the AquaScan captures are silent, the original `express.e`
(`displayFileList :27626`, `getDirSpan :26857`) fills the gap. Wire bytes
are always AquaScan's, `NextScan`-rebranded (`designs/NEXTSCAN.md` §7).
See memory `tierd-aquascan-parity-target` / `use-original-amiexpress-code`.

## Grammar

`FR` is a distinct top-level token (the original dispatch matches the
whole code `FR`, `express.e:28310`; `F R` with a space is **not** an
original reverse form and stays the D2 `Invalid`/argument-error path).
The `FR` argument grammar mirrors `F`'s captured grammar, with one
asymmetry at the bare form:

| Input | Resolves to | Source |
|---|---|---|
| `FR` (bare) | reverse scan of the **upload/highest dir**, no `Directories:` prompt | AquaScan S11 (`ae_tierd_aquascan3.txt`) |
| `FR <n>` | reverse listing of dir `n` | AquaScan S10 |
| `FR A` | reverse scan of all dirs, **highest→lowest** | `express.e` `displayFileList` reverse walk (AquaScan silent) |
| `FR U` | reverse scan of the upload/highest dir | `getDirSpan` `U` |
| `FR H` | reverse scan of the hold dir | `getDirSpan` `H` |
| `FR <span> NS` | as above, non-stop (no pager) | D2 `NS` token |
| `FR ?` | the `F ?` help screen | grammar symmetry (`'fr ?'` banner label advertises it; uncaptured — flagged) |
| other | `Argument error!` | D2 `Invalid` |

**Asymmetry, deliberate:** bare `F` opens the `Directories:` prompt;
bare `FR` skips it and scans the highest dir. This matches the AquaScan
capture and is recorded as intentional (the original `displayFileList`
would prompt bare `FR` too — overridden because AquaScan is the Tier-D
authority).

## The seam — one `reverse: bool`

Threaded parser → handler → wire, no new subsystem:

- **`menu_command.rs`** — `FileListArg::Span { span, non_stop, reverse }`
  gains the field. New parse: a leading `FR` token →
  - `FR` bare → `Span { span: FileSpan::Upload, non_stop: false, reverse: true }`
  - `FR <span> [NS]` → `Span { span, non_stop, reverse: true }`
  - `FR ?` → `FileListArg::Help`
  The `R`-is-`Invalid` doc note (`menu_command.rs:127-130`) is unwound.
  `advertised_token` still maps every `FileList` arm to `"F"` (the menu
  advertises `F`; `FR` is its sibling) so the advertises-exactly guard
  stays green.
- **`file_list/mod.rs`** — `file_list_span`/`run_span` take `reverse`.
  For `reverse`: (a) select the reverse banner, (b) reverse the dir
  iteration order for multi-dir spans (`FileSpan::All`), (c) reverse the
  per-dir `files` vec before `stream_dir_body`, (d) pass `reverse` to the
  scan header. Bare `FR` enters via `Span { Upload, reverse }`, never the
  `file_list_prompt` path.
- **`file_list/wire.rs`** —
  - `LISTING_BANNER` const → `listing_banner(reverse: bool) -> &'static [u8]`.
    Reverse variant swaps `'f ?'`→`'fr ?'` and flexes the dash run
    40→**39** to hold 77 visible cols.
  - `scanning_dir_header(n, found)` → `scanning_dir_header(n, found, reverse)`.
    Reverse text: `Reverse-scanning dir N... Ok!` /
    `Reverse-scanning dir N... Nothing found!` (note: no "from top").

Forward callers pass `reverse = false` and are byte-unchanged (pinned by
the existing D2 tests).

## Wire targets (byte-pinned)

From `comparison/transcripts/ae_tierd_aquascan3.txt` S10 (`FR 1`) and S11
(bare `FR`):
- Banner: `--[ NextScan ]---------------------------------------[ 'fr ?' for options ]--` (39 dashes, 77 cols).
- Header: `Reverse-scanning dir N... Ok!`.
- Per-dir files newest-first — literally the `find_in_area` vec reversed
  (forward `F 2`: FRESHUPL→MYDEMO→TOOLPACK; `FR`/`FR 2`: reversed).
- Frames, colour fields, `[ End of File List ]`, `More?` pager, exit
  tails: identical to D2 (unchanged code path).

## Test plan (TDD, failing test first)

1. **Parser** (`menu_command.rs` tests): the existing
   `parse_menu_command("FR")`/`("FR 1")` → `Unknown` assertions
   (`menu_command.rs:1138-1139`) flip to
   `FileList(Span { Upload, .., reverse:true })` and
   `FileList(Span { Dir(1), .., reverse:true })`. Add `FR A`/`FR U`/`FR H`/
   `FR 1 NS`/`FR ?` cases. **This is the first failing test.**
2. **Wire** (`wire.rs` tests): `listing_banner(true)` holds 77 visible
   cols and carries `'fr ?'`; `scanning_dir_header(n, found, true)`
   bytes == `Reverse-scanning dir N... Ok!`/`Nothing found!`.
3. **Handler** (`file_list/tests.rs`): a reverse span emits files
   newest-first (reversed corpus order) and the reverse banner/header;
   `FR A` descends dirs highest→lowest; forward `F` output unchanged.
4. **Telnet smoke** (the D-wire closing slice,
   `tests/tierd_file_list_smoke.rs`): `FR 1` and bare `FR` against the
   running binary reproduce the captured bytes (banner, header,
   newest-first body).
5. **Mutation:** `cargo mutants --file src/app/menu_flow/file_list/wire.rs`
   and `--file src/app/menu_command.rs` (+ `--in-diff main...HEAD`); the
   `reverse` branch selections must be caught.

## Out of scope / deferred

- `F R` (space modifier) — AquaScan-help-only grammar, uncaptured;
  stays the D2 `Invalid` path.
- Sorting on fields other than upload-date (D3 slice doc out-of-scope).
- `FR ?` exact help bytes if a capture later shows a distinct `'fr ?'`
  help screen (currently reuses `F ?`).
- The `Q`uick-scan token (uncaptured for `F`, ditto `FR`).
