---
name: qa
description: "QA the FLOW plugin locally. Switch marketplace to local source, test in a live session, restore when done."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — requires the plugin to be installed.

## Announce

Print:

```
============================================
  FLOW QA — STARTING
============================================
```

## Step 1 — Gate on bin/ci

Run `bin/ci`. If it fails, stop:

> "bin/ci failed. Fix the failures before QA testing."

## Step 2 — Switch marketplace to local source

Run:

```bash
claude plugin marketplace add <project_root>
claude plugin marketplace update flow-marketplace
```

## Step 3 — Wait for manual testing

Print:

> Dev mode active. The plugin cache now contains this source repo's files.
>
> Open a **new** Claude Code session in a target project to test.
>
> Return here when done.

Then ask:

```
AskUserQuestion: "Did QA pass?"
  - "Yes — QA passed"
  - "No — QA failed"
  - "Not done yet — keep testing"
```

If **"Not done yet"**: re-ask the same question (loop until Yes or No).

## Step 4 — Restore production marketplace

Run:

```bash
claude plugin marketplace add benkruger/flow
claude plugin marketplace update flow-marketplace
```

## Step 5 — Report

If QA passed:

```
============================================
  FLOW QA — PASSED
============================================
```

If QA failed:

```
============================================
  FLOW QA — FAILED
============================================
```
