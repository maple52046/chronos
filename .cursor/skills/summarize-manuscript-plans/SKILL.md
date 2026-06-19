---
name: summarize-manuscript-plans
description: Consolidate the AI plan manuscripts under docs/plans/manuscripts/ into a single long-term plan following docs/plans/manuscripts/README.md, then delete the original draft manuscripts (never README.md). Use when the user runs /summarize-manuscript-plans or asks to consolidate, summarize, or roll up the plan manuscripts.
disable-model-invocation: true
---

# Summarize Manuscript Plans — Cursor Entry

This is the Cursor-specific entry point for the `summarize-manuscript-plans`
skill. Its only job is to wire the skill into Cursor's `/`-command discovery.

The full, IDE-neutral instructions are defined once in:

`skills/summarize-manuscript-plans/SKILL.md` (relative to the repository root).

When this skill is invoked, read `skills/summarize-manuscript-plans/SKILL.md` and
follow it exactly. Do not duplicate or fork the steps here — keep this file as a
thin reference so there is a single source of truth.
