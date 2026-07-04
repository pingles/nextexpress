# Stage 3 — Design workflow shape

Load when running Stage 3. This is the **full-rigour Workflow** that runs *inside* the
stage and returns **one synthesized design**; the human GATE fires only **after** the
workflow returns. Prompts per role live in `subagent-briefs.md`; where the output goes
and its literal/encoding rules live in `artifact-conventions.md`; the invariants cited
below (§10.x) are detailed in `hardening.md`.

## Inputs / output

- **In:** the Stage-2 evidence — `comparison/transcripts/<cmd>.txt`, the
  `live-observations.md` note, the tagged stable-vs-volatile fields (§10.6), the flagged
  interactive/pager/hotkey rows (§10.7); the Allium specs (`specs/*.allium`); the legacy
  dispatch line (`amiexpress/express.e:28285`) + relevant control-flow procs.
- **Out:** `designs/YYYY-MM-DD-<cmd>-design.md` — one synthesized, refuted, reconciled
  design with an implementation plan. Then → **GATE**.

## Model map (authoritative)

| Role | Model | Effort |
|---|---|---|
| 2–3 candidate designers (distinct framings) | **Fable** | high |
| Judge panel (scores candidates) | **Opus** | high |
| Synthesizer (winner + graft runners-up) | **Opus** | high |
| Adversarial refuter(s) | **Opus** + one **Fable** | **max** (hardest pass) |
| Authority reconciler (door-shadowed only) | **Opus** | high |

Fable does the generative design work; Opus judges, refutes, and reconciles.

## The workflow, end to end

1. **Fan out designers (parallel).** 2–3 **Fable** designers, one framing each — do not
   let them converge on the same shape:
   - **minimal-change** — smallest diff that satisfies the captures; reuse existing seams.
   - **cleanest-seam** — the most idiomatic hex-arch port/adapter boundary.
   - **closest-to-legacy** — mirror `express.e` control-flow / dispatch semantics most
     faithfully.
   Each emits a candidate design keyed to the Stage-2 grammar rows and the Allium spec.

2. **Judge panel (Opus).** Score every candidate on the five facets, each independently:

   | Facet | Question |
   |---|---|
   | Allium conformance | Does it satisfy `specs/*.allium` obligations for this command? |
   | Capture-parity (Stage 2) | Does every grammar row + edge row map to a handling? |
   | Hex-arch cleanliness | Domain isolated from adapters? seams idiomatic, not forced? |
   | Test-first feasibility | Can each behaviour be pinned by a failing test first? |
   | Blast radius | How many modules/shared docs move; regression surface? |

   Judges emit per-facet scores + rationale; no single judge owns the verdict.

3. **Synthesize (Opus).** Pick the winner by aggregate score, then **graft** the best
   ideas from the runners-up (e.g. minimal-change's diff shape onto cleanest-seam's
   boundary). Produce one merged design.

4. **Adversarial refutation (max — Opus + one Fable).** Skeptics try to *break* the
   synthesized design — this is where source-derived guesses have historically been wrong:
   - **Violates Allium?** point to the specific obligation it breaks.
   - **Misses a captured edge row?** name the transcript row with no handling (empty/junk,
     out-of-range + non-numeric, unknown token, trailing junk, bare-CR/LF, lone-key vs
     line-read, sub-prompt accept/reject, empty-collection gate) — §10.2.
   - **Unjustified abstraction (§10.4)?** a new port/adapter **enum arm** must cite a
     Stage-2 behaviour *unreachable* with existing seams; the refuter tries to show it *is*
     reachable without it (cf. the deleted `Silent` EchoMode arm). No premature abstraction.
   Survivors feed back to synthesis. **Capped (§10.8):** ≤ 2 refute→revise rounds; on a
   third unresolved objection, escalate to the gate — never spin.

5. **Authority reconciliation — door-shadowed tokens only (§10.3, high).** For
   `F/FR/N/SCAN/NSU/CS/SENT` the AquaScan door capture and `express.e` genuinely
   conflict. **Diff** the door capture against the `express.e` dispatch/control-flow.
   - **On any divergence: HALT.** Surface it at the gate as an explicit **A/B decision**
     (express.e-wins default). A subagent **never** auto-resolves this.
   - Tag authority **per facet:** `bytes:capture` (AquaScan owns wire bytes) vs
     `control-flow:express.e:NNNN` (source owns silent control-flow) — the NextScan rebrand
     mixes both inside one token. Record captured-vs-extrapolated in the design doc +
     `COMMAND_PARITY.md`.

6. **Write `designs/YYYY-MM-DD-<cmd>-design.md`** (template below), then return it. The
   stage's human GATE fires here.

## Workflow-script sketch (adapt per run)

```js
// Stage 3 — returns ONE synthesized design; gate fires after this resolves.
// model/effort are pinned per the map above.

const framings = ["minimal-change", "cleanest-seam", "closest-to-legacy"];

const design = await pipeline(
  // 1. designers fan out in parallel — Fable, distinct framings
  async (ctx) => parallel(
    framings.map((f) =>
      agent({
        role: `designer:${f}`,
        model: "fable",
        effort: "high",
        brief: briefs.stage3Designer(f, ctx), // subagent-briefs.md
      })
    )
  ),

  // 2. judge panel — Opus, five facets, independent
  async (candidates) => agent({
    role: "judge-panel",
    model: "opus",
    effort: "high",
    brief: briefs.stage3Judge(candidates, {
      facets: ["allium", "capture-parity", "hex-arch", "test-first", "blast-radius"],
    }),
  }),

  // 3. synthesize winner + graft runners-up — Opus
  async (scored) => agent({
    role: "synthesizer",
    model: "opus",
    effort: "high",
    brief: briefs.stage3Synthesize(scored),
  }),

  // 4. adversarial refutation — max; loop capped at 2 (§10.8)
  async (winner, ctx) => {
    let design = winner;
    for (let round = 0; round < 2; round++) {
      const objections = await parallel([
        agent({ role: "refuter:opus",  model: "opus",  effort: "max",
                brief: briefs.stage3Refute(design, ctx) }),
        agent({ role: "refuter:fable", model: "fable", effort: "max",
                brief: briefs.stage3Refute(design, ctx) }),
      ]);
      if (objections.every((o) => o.clean)) return design;
      design = await agent({ role: "synthesizer", model: "opus", effort: "high",
                             brief: briefs.stage3Revise(design, objections) });
    }
    return escalateToGate(design, "refutation cap hit"); // §10.8 / §10.10
  },

  // 5. authority reconciliation — door-shadowed tokens only; HALT on divergence
  async (design, ctx) => {
    if (!isDoorShadowed(ctx.cmd)) return design;           // F/FR/N/SCAN/NSU/CS/SENT
    const conflict = await agent({
      role: "authority-reconciler", model: "opus", effort: "high",
      brief: briefs.stage3Authority(design, ctx),          // diff capture vs express.e
    });
    if (conflict.diverges) return haltForAB(conflict, { default: "express.e-wins" }); // §10.3
    return design;
  },
);

await writeDesignDoc(design); // designs/<date>-<cmd>-design.md — then GATE
```

Escalation on model failure (§10.10): a designer/refuter that won't produce a compilable
plan → retry once → escalate the failing role Fable→Opus → halt to the gate. The
least-bad candidate never ships silently.

## Design-doc template outline

`designs/YYYY-MM-DD-<cmd>-design.md`:

1. **Command + track.** Token, dispatch line `express.e:28285`, user-facing (six-stage) vs
   refactor track.
2. **What changes.** Modules/seams touched, blast radius (from the judge facet).
3. **Allium conformance.** Which `specs/*.allium` obligations this satisfies, cited.
4. **Grammar table (§10.4) — every input form.** The adversarial pass asserts no accepted
   form is missing a row; Stage 5 runs one scenario per row.

   | Input form | Capture ref | Authority (per facet) | Intended handling |
   |---|---|---|---|
   | bare `<cmd>` (CR) | `transcripts/<cmd>.txt:LL` | bytes:capture / control-flow:`express.e:NNNN` | … |
   | `<cmd> <inline-arg>` | `…:LL` | bytes:capture / control-flow:`express.e:NNNN` | … |
   | sub-prompt `<name>` accept set | `…:LL` | bytes:capture | … |
   | sub-prompt reject set (out-of-range / non-numeric / unknown) | `…:LL` | control-flow:`express.e:NNNN` | … |
   | empty / whitespace / bare-CR / bare-LF | `…:LL` or *extrapolated* | control-flow:`express.e:NNNN` | … |
   | trailing junk after valid arg | `…:LL` | bytes:capture | … |
   | empty-collection gate path | `…:LL` | control-flow:`express.e:NNNN` | … |

   Each row is tagged **per facet**: `bytes:capture` (what the wire showed) and
   `control-flow:express.e:NNNN` (source-owned silent flow). Rows resolved from source
   because the edge is uncapturable are marked *extrapolated-from-source* (§10.2).

5. **Volatile vs stable fields (§10.6).** Which captured fields byte-pin (glyphs, prompts,
   dash geometry) vs assert by derivation (dates, times, node/conf numbers, last-call
   defaults).
6. **Encoding surfaces (§10.7).** Each byte ≥0x80: Latin-1 byte + target UTF-8 code point;
   the `COMMAND_PARITY.md` departure row it needs.
7. **Ports/adapters.** Any new seam, with the Stage-2 behaviour that makes it necessary
   (§10.4 no-premature-abstraction); modelled-error path for every port/store touch.
8. **Door/source reconciliation (door-shadowed only, §10.3).** The capture-vs-`express.e`
   diff, the A/B decision taken at the gate, captured-vs-extrapolated tags.
9. **Implementation plan.** Ordered TDD steps for Stage 4 — each a failing test pinned to a
   capture literal (with `express.e:N` provenance) → minimum code, plus the
   `COMMAND_PARITY.md` / `SYSTEM.md` edits to make.
