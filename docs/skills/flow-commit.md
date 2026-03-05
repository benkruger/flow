---
title: /flow:commit
nav_order: 2
parent: Skills
---

# /flow:commit

**Phase:** Any

**Usage:** `/flow:commit` or `/flow:commit --auto`

Reviews all pending changes before committing. You see the full diff and proposed commit message, then approve or deny before anything is pushed. This is the only way commits are made in the FLOW workflow.

---

## What It Does

1. Runs `git status` and `git diff HEAD` separately and displays the diff
2. Proposes a commit message in the `tl;dr` format
3. Asks for explicit approval before touching git
4. On approval — `git add -A`, commits with the proposed message, pushes
5. On denial — asks what needs to be fixed and stops

---

## Commit Message Format

```text
Short subject line (imperative verb, under 72 characters)

tl;dr

One or two sentences explaining the WHY.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
```

Subject starts with an imperative verb — Add, Fix, Update, Remove, Refactor. No prefix jargon.

---

## Modes

Commit auto-detects its context:

| Mode | When | Banner | Python auto-approval |
|------|------|--------|---------------------|
| FLOW | State file exists | Versioned (`FLOW v0.14.0 — flow:commit`) | Yes (via `.flow.json`) |
| Maintainer | No state file, `flow-phases.json` exists | Plain (`Commit`) | No |
| Standalone | No state file, no `flow-phases.json` | Plain (`Commit`) | No |

All three modes share the same diff/message/approval/push process.

---

## Gates

- Never commits without showing the diff first
- Never skips the approval step — unless `--auto` or Python framework (FLOW mode only)
- Never uses `--no-verify`
- FLOW mode: Warns if `bin/ci` has not been run since the last code change

---

## Auto Mode

Pass `--auto` to skip the approval prompt when you already know the change is good:

```text
/flow:commit --auto
```

Everything else stays identical: `bin/ci` runs first, the full diff is displayed, the commit message is generated and shown, and pull-before-push happens. The only difference is that Step 3 (approval prompt) is skipped.

**Python projects (FLOW mode only)** automatically use auto mode — when the target project's framework is `python` (per `.flow.json`), the approval prompt is always skipped. This applies only in FLOW mode; Maintainer and Standalone modes always require explicit approval.

`--auto` is user-invoked only. Claude must never call `/flow:commit --auto` programmatically.
