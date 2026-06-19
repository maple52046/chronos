# Repository Agent Skills

This document is the entry point that directs AI agents to the skills available
in this repository. All paths are relative to the repository root.

A skill is a task-specific operating guide. Read a skill only when the current
task matches its purpose.

## Skill Layout

All skills for this repository live under `skills/`. Each skill is a folder
containing a `SKILL.md` (plus optional `scripts/` or `references/` resources),
and must be listed in the Skill Index below.

## Skill Index

| Skill | Path | When to Use |
| --- | --- | --- |
| Build Container Image | `skills/build-container-image/SKILL.md` | When the task needs to build the Chronos container image from the repository `Dockerfile`, tag it as `${repo}:${version}-${ts}`, and optionally push it. Triggered by `/build-container-image`. |
| Git Commit | `skills/git-commit/SKILL.md` | When the task needs to analyse the repository's changes, draft a Conventional Commits message, and optionally run `git commit` / `git push`. Triggered by `/git-commit`. |
| Summarize Manuscript Plans | `skills/summarize-manuscript-plans/SKILL.md` | When the task needs to consolidate, summarize, or roll up the AI plan manuscripts under `docs/plans/manuscripts/` into a single long-term plan (following `docs/plans/manuscripts/README.md`) and then delete the original draft manuscripts (never `README.md`). Triggered by `/summarize-manuscript-plans`. |

## Adding a New Skill

For every new skill:

1. Create a folder named after the skill containing a `SKILL.md` (for example
   `skills/inspect-logs/SKILL.md`). Put any helper scripts or resources inside
   that folder.
2. Use a clear, action-oriented title and open with a short "When to Use This
   Skill" section so agents can quickly decide whether the skill applies.
3. Keep all paths inside the skill relative to the repository root.
4. Add a row to the Skill Index above with the skill title, its path, and a
   one-sentence "When to Use" description.

## IDE Integration (Wrapper Convention)

Skills are IDE-neutral. The canonical instructions (and any helper scripts) for
each skill live once in `skills/<name>/SKILL.md`; that file is the single source
of truth. Individual `SKILL.md` files MUST NOT restate this convention — it is
defined here only.

Each IDE adds a thin wrapper that *references* the canonical file instead of
duplicating it:

- **Cursor**: `.cursor/skills/<name>/SKILL.md` carries Cursor frontmatter
  (`name`, `description`) and points back to `skills/<name>/SKILL.md` so
  `/<name>` works as a slash command. Always use this folder form — never a flat
  `.cursor/skills/<name>.md`, and never drop the `skills/` path segment.
- **Other IDEs / agents**: register the skill wherever the tool scans and
  reference `skills/<name>/SKILL.md` rather than copying it.
