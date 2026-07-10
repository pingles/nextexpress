# subagent-briefs.md — stage → agent dispatch index

Every role is a versioned **agent definition** in the active client's agent directory:
`.claude/agents/cs-*.md` for Claude Code or `.codex/agents/cs-*.toml` for Codex. The agent file
**is** the role's brief — this file is only the index. Dispatch each named role with the active
client's native subagent mechanism and preserve its configured model and reasoning effort.

Deep per-stage shapes: `stage3-design.md`, `stage5-comparison.md`. Board driving:
`board-lifecycle.md`. Edge checklist: `edge-probe-battery.md`. Write-locations + literal rules:
`artifact-conventions.md`. The §10 invariants: `hardening.md`.

## Dispatch table

| Stage | Role | Agent | configured tier | What it does |
|---|---|---|---|---|
| 1 | assessment readers (roadmap+git / refactor-scan / Allium-drift) | `cs-assess-reader` | assessment / medium | read-only Stage-1 state read; focus per task; run-state resume check |
| 1 | prereq critic | `cs-prereq-critic` | assessment / high | bounded adversarial check on the plan → PASS/AMEND · **GATE after** |
| 2 | capture driver | `cs-capture` | assessment / high | drive live FS-UAE; write harness / transcript / evidence note |
| 2 | completeness critic | `cs-completeness-critic` | assessment / high | edge-probe-battery coverage; bounded re-probe; express.e fallback |
| 3 | designers ×2–3 (framings) | `cs-designer` | generative / high | candidate design + grammar table; one framing per dispatch |
| 3 | judge + synthesize | `cs-judge` | assessment / high | score the 5 facets, synthesize winner + graft |
| 3 | adversarial refuters | `cs-refuter` | assessment / xhigh | prove the design wrong; use a separately configured generative role only when available |
| 3 | authority reconciler (door-shadowed only) | `cs-authority` | assessment / high | diff door capture vs express.e; halt divergence · **GATE after** |
| 4 | implementer (user-facing + refactor modes) | `cs-implementer` | generative / high | test-first build; §10 discipline; §10.10 escalation |
| 4 | post-build reviewer | `cs-reviewer` | assessment / high | mutation-gap · capture-parity + provenance · Allium-drift · doc-staleness |
| 5 | scenario author | `cs-scenario` | assessment / high | target-agnostic scenario set (one per grammar row + edges) |
| 5 | Tester-A (NextExpress) | `cs-tester-next` | assessment / high | char-at-a-time NextExpress session log (blind to reference) |
| 5 | Tester-B (FS-UAE, **serialized**) | `cs-tester-ref` | assessment / high | live board session log; serialized; clean `G Y`; budget < 5 |
| 5 | cross-markers (double-blind) | `cs-crossmark` | assessment / xhigh | mark the *other* tester's log; flag divergences · **GATE after** |
| 5 | completeness critic | `cs-completeness-critic` | assessment / high | "what did both testers fail to exercise?" |
| 6 | root-cause triage (one per divergence) | `cs-triage` | assessment / high | classify divergence; confirm reference ambiguity · **GATE: ask user** |

Stages 3 and 5 orchestrate these roles for the fan-out / loop structure; every other stage
dispatches its agents directly. The human **GATE**s fire in the orchestrator (`SKILL.md`), after
the stage's agents return.
