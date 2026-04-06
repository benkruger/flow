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

1. Runs CI (FLOW-enabled mode) and stages changes in parallel
2. Shows `git status` and `git diff --cached` in parallel
3. Proposes a commit message in the `tl;dr` format
4. Commits, pulls, and pushes via `bin/flow finalize-commit`

---

## Commit Message Format

The format is determined by the `commit_format` setting in `.flow.json`, chosen during `/flow-prime`.

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

## Modes

Commit auto-detects its context:

| Mode | When | CI | Banner |
|------|------|----|--------|
| FLOW-enabled | `.flow.json` exists | Runs `bin/flow ci` | Versioned if state file exists, plain otherwise |
| Standalone | No `.flow.json` | Skips CI | Plain (`Commit`) |

Both modes share the same diff/message/push process.

---

## Gates

- Never commits without showing the diff first
- Never uses `--no-verify`
- FLOW-enabled mode: Runs `bin/flow ci` before the diff — skipped in Standalone mode
- FLOW mode: Warns if `bin/flow ci` has not been run since the last code change
