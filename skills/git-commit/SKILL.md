---
name: git-commit
description: >-
  Analyse the repository's changes, write a Conventional Commits message, and run
  git commit (optionally git add and git push). Use when the user runs
  /git-commit or asks to commit staged changes with a generated message.
---

# git-commit

Analyse the repository's changes, write a high-quality commit message, and run `git commit`.
**By default, only the files the user has already staged are processed**; commit only, no push.

## Invocation

```
/git-commit [--auto-add] [--all] [--push] [--date <when>]
```

| Option | Required | Description |
| --- | --- | --- |
| `--auto-add` | No | Allow the agent to select **relevant** files and `git add` them itself. Without it, **only files already staged by the user are processed**; the agent must not add files on its own. |
| `--all` | No | Allow the agent to run `git add -A` directly, staging **all** changes in the working tree (modified / added / deleted / untracked) at once before committing. More permissive than `--auto-add` and non-selective; when used together with `--auto-add`, `--all` takes precedence. |
| `--push` | No | Run `git push` after a successful commit. Without it, **commit only, no push**. |
| `--date <when>` | No | Set the commit date, with **author date and committer date both set to the same value** (kept consistent). `<when>` uses a git-parseable format, e.g. `'12 hours ago'`, `'2026-06-10 13:00 +0800'`. |

## Workflow

```
- [ ] 1. Gather current state in parallel: git status / git diff (staged) / git log
- [ ] 2. Determine the commit scope by mode (staged-only / --auto-add / --all)
- [ ] 3. Check for secrets and files that should not be version-controlled
- [ ] 4. Write the commit message per the Conventional Commits spec
- [ ] 5. Run git commit (pass the message via HEREDOC; keep author/committer date consistent when --date is given)
- [ ] 6. --push: run git push after a successful commit
- [ ] 7. Verify and report the results
```

### 1. Gather current state (run in parallel)

Run these three commands in parallel at once to understand what to commit and the repo's message style:

- `git status` — view staged / unstaged / untracked.
- `git diff --staged` — view the actual staged changes (for auto-add mode, see step 2 as well).
- `git log --oneline -15` — observe the repo's existing Conventional Commits usage (common types, scope naming, language) and follow the established conventions.

### 2. Determine the commit scope (mode selection)

- **Default (no `--auto-add` / `--all`)**: commit only the currently staged files.
  - If **there are no staged files at all**: stop and report, prompting the user to `git add` first, or to use `--auto-add` / `--all`. **Do not add files yourself.**
  - Unstaged / untracked changes in the working tree are normal and are not included in this commit.
- **`--auto-add`**: you may add relevant files to staging yourself.
  - Run `git diff` (unstaged) and inspect untracked files, then `git add` the files **relevant** to this change.
  - Avoid including unrelated temporary files, build artifacts, logs, etc.; respect `.gitignore`.
- **`--all`**: run `git add -A` directly at the repo root to stage **all** working-tree changes (modified / added / deleted / untracked) at once.
  - No selection, but **the secrets check (step 3) still applies**: still do not include `.env`, private keys, tokens, artifacts / logs, etc.; anything already covered by `.gitignore` will not be added.
  - When used together with `--auto-add`, `--all` takes precedence (more permissive).

### 3. Secrets and safety check

- Do not commit files suspected of containing secrets (`.env`, `credentials.json`, private keys, tokens, etc.). If such files are included, stop and warn the user.
- Follow git safety practices: do not modify git config; do not use the `-i` interactive flag; do not add `--no-verify`.

### 4. Write the commit message (follow Conventional Commits)

The commit message **must** follow the Conventional Commits 1.0.0 spec. For the full text, see
[`knowledge/development/conventional-commit/`](conventional-commits-1.0.0.md); when unsure about details (footers,
breaking-change determination), always go back and check. Format:

```
<type>[(scope)][!]: <description>

[body]

[footer(s)]
```

- **type** (required): a noun prefix followed by `: ` (colon and a space). `feat` = a new feature, `fix` = a bug fix; other common ones: `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `style`.
- **scope** (optional): a noun in parentheses marking the area of impact, e.g. `fix(parser):`; follow the existing scope naming in `git log`.
- **description** (required): a concise summary immediately after `: ` (recommended ≤ 72 characters), focused on the change itself, in the imperative mood.
- **body** (optional): separated from the description by **one** blank line; freely explain the motivation and impact (the why).
- **footer(s)** (optional): separated from the body by one blank line; tokens use `-` in place of spaces (e.g. `Reviewed-by:`, `Refs: #123`).
- **breaking change**: add `!` after the type/scope and before the `:` (e.g. `feat(api)!:`), or write an uppercase `BREAKING CHANGE: <description>` in a footer; either one suffices, and the footer may be omitted when `!` is already present.
- type/scope are case-insensitive, but `BREAKING CHANGE` **must** be uppercase.
- Do not claim in the message that tests were run or add markers that were not requested.

### 5. Run git commit

Always pass the message via HEREDOC to ensure correct formatting:

```bash
git commit -m "$(cat <<'EOF'
<type>(<scope>): <description>

<optional body>

<optional footers>
EOF
)"
```

**`--date <when>`: keep author/committer date consistent.** `git commit --date` only changes the author date;
the committer date is controlled separately by the `GIT_COMMITTER_DATE` environment variable. When backdating, pass **the same value** to both:

```bash
GIT_COMMITTER_DATE='<when>' git commit --date='<when>' -m "$(cat <<'EOF'
<type>(<scope>): <description>
EOF
)"
```

- `<when>` uses a git-parseable format (e.g. `'12 hours ago'`, `'2026-06-10 13:00 +0800'`).
- Only when `GIT_COMMITTER_DATE` and `--date` use the same value will the resulting commit's author/committer dates be consistent.
- This only affects the commit being **newly created** this time; it **does not rewrite existing history**.

- If there is a pre-commit hook:
  - hook fails → fix the problem and **create a new commit** (do not `--amend`).
  - hook passes but auto-modifies files → `git add` the changes, then `--amend` to fold them into this commit.

### 6. push (only with `--push`)

- Only run this when `--push` is given. First check whether the current branch already tracks a remote:
  - Already tracking: `git push`.
  - Not tracking: `git push -u origin HEAD`.
- Do not force push to main/master; warn the user before any force push.

### 7. Verify and report

- After committing, run `git status` to confirm success; with `--date`, use `git log -1 --pretty=fuller` to confirm AuthorDate and CommitDate match.
- Report: the commit summary, the files included (in `--auto-add` / `--all` mode, specifically list the files added by the agent), the date applied (if `--date` was given), and whether it was pushed and to which branch.

## Important reminders

- When there is nothing to commit (no staged changes and not `--auto-add` / `--all`, or no changes at all), do not create an empty commit; stop and report.
- The default behavior minimizes side effects: **commit staged files only, no push, use the current time**; `--auto-add` / `--all` (loosen file staging), `--push` (push), and `--date` (specify the date) relax this.
