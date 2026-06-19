# Summarize Manuscript Plans

Consolidate the AI plan manuscripts under `docs/plans/manuscripts/` into a single
long-term plan, following the consolidation spec in
`docs/plans/manuscripts/README.md`, then delete the original draft manuscripts.

Run all commands from the repository root.

## When to Use This Skill

Read this skill when the task asks to consolidate, summarize, or roll up the AI
plan manuscripts under `docs/plans/manuscripts/` — especially when the user runs
`/summarize-manuscript-plans`.

## Inputs and Outputs

| | Path |
| --- | --- |
| Source manuscripts | `docs/plans/manuscripts/*.md` (every file **except** `README.md`) |
| Consolidation spec | `docs/plans/manuscripts/README.md` |
| Consolidated output | `docs/plans/<yyyyMMDD_HHMM>_<title>.md` (the parent of `manuscripts/`) |

## Workflow

1. **Read the spec.** Open `docs/plans/manuscripts/README.md` and treat it as the
   authoritative consolidation spec: the required sections, the filename rule, and
   the consolidation rules (do not concatenate; prefer the newer or more explicit
   decision on conflict; unresolved conflicts go under Open Questions; do not
   introduce large new designs the sources do not support).
2. **Collect the sources.** Read every `*.md` under `docs/plans/manuscripts/`
   **except** `README.md`. If there are no such files, stop and report that there
   is nothing to consolidate — do not write or delete anything.
3. **Consolidate.** Synthesize one coherent plan (not a concatenation) that
   preserves planning content, architectural decisions, constraints and rules,
   implementation boundaries, confirmed user preferences, and open questions. Use
   these sections, in order:
   1. Purpose
   2. Source Scope
   3. Consolidated Background
   4. Confirmed Decisions
   5. Architecture and Design Principles
   6. Functional Scope
   7. Constraints and Rules
   8. Data Model and Format Notes
   9. CLI / API / Config Notes
   10. Implementation Plan
   11. Non-goals
   12. Open Questions
   13. Future Work
4. **Name and write the output.** Write to `docs/plans/` (the parent of
   `manuscripts/`) using `yyyyMMDD_HHMM_<title>.md`, where `yyyyMMDD_HHMM` is the
   current execution time and `<title>` is lowercase snake_case describing the
   dominant topic. If the topic is unclear, use
   `yyyyMMDD_HHMM_consolidated_ai_plans.md`. Get the timestamp from
   `date +%Y%m%d_%H%M`.
5. **Verify the output.** Confirm the consolidated file exists and contains all 13
   sections built from the sources. Do not proceed to deletion until it is written
   and verified.
6. **Delete the originals.** Only after the consolidated file is verified, delete
   the source draft manuscripts — every `*.md` under `docs/plans/manuscripts/`
   **except** `README.md`. Never delete `README.md`, and never delete the
   consolidated output (it lives in `docs/plans/`, not in `manuscripts/`).
7. **Report**:
   - the generated file path
   - the source directory
   - the number of files processed
   - the main themes consolidated

## Important

- This skill **intentionally deletes the original drafts** in step 6. That is the
  one place it overrides the spec's "Do not modify, delete, or move the original
  draft files" rule — which applies only **during** consolidation so the summary
  is built from intact sources. Deletion happens **only after** the consolidated
  file is written and verified.
- `docs/plans/manuscripts/README.md` is the spec, not a draft: never read it as a
  source plan and never delete it.
- This skill consolidates existing plans only; it never invents new designs that
  the source manuscripts do not support.
