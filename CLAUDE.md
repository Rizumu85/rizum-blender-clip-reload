# CLAUDE.md

Project-level instructions for Claude.

## Working Agreement

- Rizum Guidelines are active for this project/thread until the user says otherwise.
- Karpathy Guidelines are active for this project/thread until the user says otherwise.
- Start with `docs/AI_MEMORY.md` for the current renderer state before reading the long research logs.
- Write project files, code comments, and technical docs in English. Summarize chat changes in Chinese.

## Doc Roles

- `docs/AI_MEMORY.md`: current compact state for agents.
- `docs/analysis.md`: append-only technical findings, native evidence, and rejected hypotheses.
- `docs/design.md`: Blender UX and user-facing behavior.
- `docs/plan.md`: directions, completed work, and current open tasks.

## Engineering Rules

- Read the relevant docs before changing code.
- Keep changes surgical and traceable to the requested task.
- Prefer existing patterns in `clip_loader.py` and `clip_studio_importer/clip_loader.py`.
- Do not replace importer behavior from one reverse-engineering clue unless a targeted sample improves and guard samples stay stable.
- Treat metric-only probes as diagnostic until native evidence supports them.
- Preserve historical rejection notes in `docs/analysis.md`; update `docs/AI_MEMORY.md` when the current state changes.

## Verification

Run targeted checks only when the change needs them. For the current vector-native work, the usual guard set is:

```powershell
python verify_one_clip.py img/Vector_SizePressure.clip
python verify_one_clip.py img/Vector_OpacityPressure.clip
python verify_one_clip.py img/Vector_OpacityRandom_50.clip
python verify_one_clip.py img/Vector_FlowPressure_50.clip
python verify_one_clip.py img/Vector_Flow_50.clip
python verify_one_clip.py img/Vector_Baseline.clip
```
