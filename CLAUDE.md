# CLAUDE.md

Project-level instructions for Claude. Combines two behavioral guideline sets: **Karpathy Guidelines** (avoid common LLM coding mistakes) and **Rizum Guidelines** (planning, handoff docs, clear communication).

## Working Agreement

- Rizum Guidelines are active for this project/thread until the user says otherwise.
- Karpathy Guidelines are active for this project/thread until the user says otherwise.
- For trivial tasks, use judgment — these guidelines bias toward caution and deliberate planning over speed.

---

# Part A — Karpathy Guidelines

Behavioral guidelines to reduce common LLM coding mistakes.

## A1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

## A2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Self-check: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

## A3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it — don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: every changed line should trace directly to the user's request.

## A4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria enable independent looping. Weak criteria ("make it work") require constant clarification.

---

# Part B — Rizum Guidelines

Behavioral guidelines for planning, handoff docs, and clear user communication.

## B1. Persist Activation

**If Rizum is active, write it down where future agents will read it.**

- Treat Rizum Guidelines as active for this project/thread until the user says otherwise.
- The activation line at the top of this file (`## Working Agreement`) is the canonical record.
- Prefer `AGENTS.md` for Codex and `CLAUDE.md` for Claude. If neither exists, add the line to `plan.md` under `Working Agreement`.
- Do not create global rules or unrelated config unless the user asks.

## B2. Ground The Work

**Read first. Create only what is missing.**

Before project code changes:
- Check for `analysis.md`, `design.md`, and `plan.md` in the project root.
- Read existing docs before writing new ones.
- Create only missing docs.
- Update stale sections before code changes.
- Never overwrite useful context just to install a template.

Doc roles:
- `analysis.md` — technical implementation analysis: API docs read, reference implementations inspected, relevant code findings, constraints, edge cases, conclusions.
- `design.md` — UI, interaction, UX, and user-facing behavior. Start with `Project Goal` in plain language.
- `plan.md` — high-level directions with concrete implementation steps under each direction.

## B3. Plan In Two Layers

**Direction first. Steps second.**

Structure `plan.md` like this:

```md
## Working Agreement

- Rizum Guidelines are active for this project/thread until the user says otherwise.

## Direction 1: Short Name

Goal: One sentence describing this direction.

- [ ] Concrete implementation step
- [ ] Concrete implementation step

## Direction 2: Short Name

Goal: One sentence describing this direction.

- [ ] Concrete implementation step
- [ ] Concrete implementation step
```

Good plan steps are:
- Concrete enough to verify.
- Small enough to finish independently.
- Not so tiny that the plan becomes noise.
- Grouped under the direction they serve.

## B4. Keep Docs Alive, Not Loud

**Update when direction changes. Don't narrate every keystroke.**

Update docs when:
- The high-level direction changes.
- The implementation approach changes.
- New API docs, reference code, or technical findings matter.
- User feedback changes the plan.
- A meaningful plan item is completed.

Do not update docs for every tiny thought, line edit, or local cleanup.

## B5. Let The User Test

**When the user needs to test, make it easy.**

When a change needs user verification, pause at the right moment and give a beginner-friendly handoff:
- **What changed** — one short sentence.
- **How to test** — 2–5 numbered steps.
- **Expected result** — what success looks like.
- **What to send back** — screenshot, exact error, log line, or behavior.

Avoid vague instructions like "test it". Say what to open, click, run, and observe.

## B6. Don't Run Checks By Default

**No syntax, build, or test commands unless there is a reason.**

- Do not run full project compilations or comprehensive test suites unless the user asks.
- Do not run syntax checks by default.
- Run syntax, static, build, or test commands only when the user asks, or when user feedback / debugging makes the check necessary.
- Leave functional and end-to-end testing to the user unless they explicitly ask the agent to perform it.

## B7. Communicate Bilingually

**English in files. Chinese in summaries.**

- Write project files, code, comments, `analysis.md`, `design.md`, `plan.md`, and technical docs in English.
- Summarize chat changes in Chinese.
- Keep other chat content in English when that is more natural for the task.

---

# Combined Self-Check

These guidelines are working if:
- Assumptions are surfaced before implementation, not buried inside it.
- Code changes are minimal, surgical, and traceable to the user's request.
- Tasks have explicit, verifiable success criteria.
- The project has useful `analysis.md`, `design.md`, and `plan.md` (when applicable).
- `design.md` starts with `Project Goal`; `plan.md` separates directions from steps.
- Activation is visible in agent-facing docs (this file).
- User testing handoffs are clear and beginner-friendly.
- Checks only run when requested or debug-driven.
- Change summaries in chat are in Chinese.
