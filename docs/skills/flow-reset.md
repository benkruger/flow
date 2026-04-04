---
title: /flow-reset
nav_order: 18
parent: Skills
---

# /flow-reset

**Phase:** Any (no phase gate)

**Usage:** `/flow-reset`

The nuclear option. Removes all FLOW artifacts from the current project in one
sweep — worktrees, state files, local and remote branches, and open PRs.

Must be run from the `main` branch. Inventories everything before acting and
requires explicit confirmation.

---

## What It Does

1. Checks that the current branch is `main`
2. Inventories all FLOW artifacts across five categories:
   worktrees, state files, local branches, remote branches, and open PRs
3. Displays the inventory and asks for confirmation
4. Destroys all artifacts, including start lock queue entries (best-effort — continues on individual failures)
5. Reports results and verifies cleanup

---

## When to Use It

- Multiple abandoned features have left orphaned artifacts
- You want a completely clean slate (no worktrees, no state files, no branches)
- You are starting fresh after experimenting with FLOW

---

## vs /flow-abort

| | `/flow-abort` | `/flow-reset` |
|---|---|---|
| **Scope** | Single feature | All features |
| **When** | Abandon one feature | Clean everything |
| **State file** | Required (warns if missing) | Not required |
| **Prerequisite** | Active FLOW feature | Must be on `main` |

Use `/flow-abort` to walk away from one feature.
Use `/flow-reset` to start completely fresh.

---

## Gates

- Must be on `main` branch
- Requires explicit user confirmation before any destructive action
- All operations are irreversible
