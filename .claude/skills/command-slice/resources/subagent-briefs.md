# subagent-briefs.md — stage → agent dispatch index

Every role is a versioned **agent definition** in `.claude/agents/cs-*.md` with its **model
and effort pinned in frontmatter** (see `SKILL.md` → *Dispatching subagents*). The agent file
**is** the role's brief — this file is only the index. Dispatch by `subagent_type` (Agent tool,
Stages 1/2/4/6) or `agentType` (Workflow, Stages 3/5). **Never** pass a call-time `model=` that
would override the frontmatter pin (precedence: `CLAUDE_CODE_SUBAGENT_MODEL` env → call-time
model → frontmatter → session).

Deep per-stage shapes: `stage3-design.md`, `stage5-comparison.md`. Board driving:
`board-lifecycle.md`. Edge checklist: `edge-probe-battery.md`. Write-locations + literal rules:
`artifact-conventions.md`. The §10 invariants: `hardening.md`.

## Dispatch table

| Stage | Role | Agent | model / effort | What it does |
|---|---|---|---|---|
| 1 | assessment readers (roadmap+git / refactor-scan / Allium-drift) | `cs-assess-reader` | opus / medium | read-only Stage-1 state read; focus per task; run-state resume check |
| 1 | prereq critic | `cs-prereq-critic` | opus / high | bounded adversarial check on the plan → PASS/AMEND · **GATE after** |
| 2 | capture driver | `cs-capture` | opus / high | drive live FS-UAE; write harness / transcript / evidence note |
| 2 | completeness critic | `cs-completeness-critic` | opus / high | edge-probe-battery coverage; bounded re-probe; express.e fallback |
| 3 | designers ×2–3 (framings) | `cs-designer` | fable / high | candidate design + grammar table; one framing per dispatch |
| 3 | judge + synthesize | `cs-judge` | opus / high | score the 5 facets, synthesize winner + graft |
| 3 | adversarial refuter (+ Fable co-refuter) | `cs-refuter` | opus / max | prove the design wrong; co-refuter = same agent, `model: fable` |
| 3 | authority reconciler (door-shadowed only) | `cs-authority` | opus / high | diff door capture vs express.e; halt divergence · **GATE after** |
| 4 | implementer (user-facing + refactor modes) | `cs-implementer` | fable / high | test-first build; §10 discipline; §10.10 escalation |
| 4 | post-build reviewer | `cs-reviewer` | opus / high | mutation-gap · capture-parity + provenance · Allium-drift · doc-staleness |
| 5 | scenario author | `cs-scenario` | opus / high | target-agnostic scenario set (one per grammar row + edges) |
| 5 | Tester-A (NextExpress) | `cs-tester-next` | opus / high | char-at-a-time NextExpress session log (blind to reference) |
| 5 | Tester-B (FS-UAE, **serialized**) | `cs-tester-ref` | opus / high | live board session log; serialized; clean `G Y`; budget < 5 |
| 5 | cross-markers (double-blind) | `cs-crossmark` | opus / max | mark the *other* tester's log; flag divergences · **GATE after** |
| 5 | completeness critic | `cs-completeness-critic` | opus / high | "what did both testers fail to exercise?" |
| 6 | root-cause triage (one per divergence) | `cs-triage` | opus / high | classify divergence; confirm reference ambiguity · **GATE: ask user** |

Stages 3 and 5 run these inside a **Workflow** for the fan-out / loop structure; every other
stage dispatches its agents directly. The human **GATE**s fire in the orchestrator (`SKILL.md`),
after the stage's agents return.
