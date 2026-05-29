---
title: /flow-commit
nav_order: 2
parent: Skills
---

# /flow-commit

**Phase:** Any

**Usage:** `/flow-commit`

Reviews all pending changes before committing. You see the full diff and proposed commit message before anything is pushed. This is the only way commits are made in the FLOW workflow.

---

## What It Does

1. Stages changes
2. Shows `git status` and `git diff --cached` in parallel
3. Proposes a commit message in the Conventional Commits format
4. Commits, pulls, and pushes via `bin/flow finalize-commit` (which enforces CI internally and re-stages tracked-file modifications after CI so in-place auto-fixes are captured in the same commit)

---

## Commit Message Format

FLOW commits follow the [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) specification. There is a single always-on format — no per-project choice — so downstream CHANGELOG tooling matches every merge.

```text
type(scope): description

Free-form body paragraph explaining the WHY.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why

BREAKING CHANGE: description of the incompatible change
```

The subject is `type(scope): description` — the `type` is required (`feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`), the `(scope)` is optional, and the lowercase imperative `description` has no trailing period. Append a `BREAKING CHANGE:` footer (or a `!` after the type) only when the change is backwards-incompatible.

---

## CI

CI is enforced inside `finalize-commit` itself — every commit path runs CI mechanically before `git commit`. When the CI sentinel is fresh (CI already passed for the current tree state), the check noops instantly. There is no separate CI step in the commit skill.

The banner is versioned (`FLOW v<plugin-version>`) when a state file exists at `.flow-states/<branch>/state.json`, plain (`Commit`) otherwise.

---

## Re-staging

After CI completes (and only when CI passed), `finalize-commit` runs `git add -u` to capture in-place modifications CI made to already-tracked files. This handles the canonical pattern where `bin/format` and `bin/lint` auto-fix tracked files in their default non-`CI=1` mode: without re-staging, the commit would record the pre-CI bytes from the index while CI tested the post-CI bytes in the working tree, and remote strict CI would fail on the unfixed bytes.

`git add -u` updates already-tracked files only — untracked artifacts (the commit-message file, scratch files, CI outputs the user has not yet `.gitignore`d) are NOT swept. The commit's scope stays bounded to what the user staged plus any in-place modifications CI made to those tracked files. A failed re-stage returns `step:"restage"` in the JSON envelope.

---

## Gates

- Never commits without showing the diff first
- Never uses `--no-verify`
- CI enforced inside `finalize-commit` before every commit
- Post-CI re-stage captures tracked-file modifications via `git add -u`; untracked files are not swept
