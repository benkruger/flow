---
name: flow-local-permission
description: "Promote permissions from .claude/settings.local.json into .claude/settings.json, then delete the local file."
---

# FLOW Local Permission

Promote tested permissions from `.claude/settings.local.json` into
`.claude/settings.json` and delete the local file.

## Usage

```text
/flow:flow-local-permission
```

## Steps

### Step 1 — Check for local file

Use the Read tool to read `.claude/settings.local.json`.

If the file does not exist, print:

```text
No .claude/settings.local.json found.
```

Then stop.

### Step 2 — Read settings

Use the Read tool to read `.claude/settings.json`.

### Step 3 — Compare and merge

Compare the `permissions.allow` arrays from both files.

For each entry in the local file's `permissions.allow` that is not
already present in `.claude/settings.json`'s `permissions.allow`,
add it to the allow array using the Edit tool.

### Step 4 — Delete local file

```bash
rm .claude/settings.local.json
```

### Step 5 — Report

Print a summary of what was promoted:

- If entries were promoted, list each one
- If all entries were already present, print "All entries already present."

## Rules

- Never use Bash for file reads — use the Read tool
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
