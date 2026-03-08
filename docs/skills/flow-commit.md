---
title: /flow-commit
nav_order: 2
parent: Skills
---

# /flow-commit

**Phase:** Any

**Usage:** `/flow-commit`, `/flow-commit --auto`, or `/flow-commit --manual`

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
Full-sentence subject line (imperative verb + what + why, ends with a period.)

tl;dr

One or two sentences explaining the WHY.

- path/to/file.rb: What changed and why
- path/to/other.rb: What changed and why
```

Subject starts with an imperative verb — Add, Fix, Update, Remove, Refactor. Includes the business reason. Ends with a period. No prefix jargon.

---

## Modes

Commit auto-detects its context:

| Mode | When | Banner |
|------|------|--------|
| FLOW | State file exists | Versioned (`FLOW v0.14.0 — flow:flow-commit`) |
| Maintainer | No state file, `flow-phases.json` exists | Plain (`Commit`) |
| Standalone | No state file, no `flow-phases.json` | Plain (`Commit`) |

All three modes share the same diff/message/approval/push process.

---

## Gates

- Never commits without showing the diff first
- Never skips the approval step — unless mode is **auto** (via `--auto` flag or `.flow.json` config)
- Never uses `--no-verify`
- FLOW and Maintainer mode: Runs `bin/flow ci --if-dirty` before the diff — skipped in Standalone mode
- FLOW mode: Warns if `bin/flow ci` has not been run since the last code change

---

## Auto/Manual Mode

Mode is resolved in this order:

1. `--auto` flag → auto mode (skip approval)
2. `--manual` flag → manual mode (require approval)
3. `.flow.json` `skills.flow-commit` value
4. Built-in default: **manual**

Everything else stays identical: `bin/flow ci` runs first (FLOW and Maintainer mode only), the full diff is displayed, the commit message is generated and shown, and pull-before-push happens. The only difference is whether Step 3 (approval prompt) is shown.

`--auto` is user-invoked only. Claude must never call `/flow-commit --auto` programmatically.
