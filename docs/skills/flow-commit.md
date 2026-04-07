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
3. Proposes a commit message in the `tl;dr` format
4. Commits, pulls, and pushes via `bin/flow finalize-commit` (which enforces CI internally)

---

## Commit Message Format

The format is determined by the `commit_format` setting, copied from `.flow.json` into the state file by `/flow-start`. Defaults to `"full"` when no state file exists.

**Full format** (`"full"`):

```text
Full-sentence subject line (imperative verb + what + why, ends with a period.)

tl;dr

One or two sentences explaining the WHY.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
```

**Title-only format** (`"title-only"`):

```text
Full-sentence subject line (imperative verb + what + why, ends with a period.)

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
```

Subject starts with an imperative verb — Add, Fix, Update, Remove, Refactor. Includes the business reason. Ends with a period. No prefix jargon.

---

## CI

CI is enforced inside `finalize-commit` itself — every commit path runs CI mechanically before `git commit`. When the CI sentinel is fresh (CI already passed for the current tree state), the check noops instantly. There is no separate CI step in the commit skill.

The banner is versioned (`FLOW v1.1.0`) when a `.flow-states/*.json` state file exists, plain (`Commit`) otherwise.

---

## Gates

- Never commits without showing the diff first
- Never uses `--no-verify`
- CI enforced inside `finalize-commit` before every commit
