---
name: flow-qa
description: "QA the FLOW plugin locally. Switch marketplace to local source, test in a live session, restore when done."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — requires the plugin to be installed.

## Usage

```text
/flow-qa
/flow-qa --start
/flow-qa --stop
```

- `/flow-qa` — auto-start if inactive, auto-restart if active
- `/flow-qa --start` — nuke cache, switch marketplace to local source, create `.dev-mode` marker
- `/flow-qa --stop` — nuke cache, restore production marketplace, remove `.dev-mode` marker

## Flag: `--start`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it exists, print "Already in dev mode. Use `/flow-qa --stop` to exit." and stop.

### Step 2 — Nuke cache and switch to local source

Run `git worktree list --porcelain` and read the output. The project root is the path on the first line that starts with `worktree `.

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin marketplace add <project_root>
```

Then:

```bash
claude plugin marketplace update flow-marketplace
```

### Step 3 — Create dev mode marker

Use the Write tool to create `.flow-states/.dev-mode` with the content `active`.

### Step 4 — Announce

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW QA — DEV MODE ACTIVE
============================================
```
````

Then print:

> Plugin cache now contains local source.
>
> Open a **new** Claude Code session in a target project to test.
> Run `/flow-qa --stop` when done.

## Flag: `--restart`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it does not exist, run the `--start` flow instead (skip to the `--start` section above).

### Step 2 — Nuke cache and re-register local source

Run `git worktree list --porcelain` and read the output. The project root is the path on the first line that starts with `worktree `.

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin marketplace add <project_root>
```

Then:

```bash
claude plugin marketplace update flow-marketplace
```

### Step 3 — Announce

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW QA — Cache refreshed
============================================
```
````

Then print:

> Plugin cache updated from local source.
>
> Open a **new** Claude Code session in a target project to test.

## Flag: `--stop`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it does not exist, print "Not in dev mode. Nothing to stop." and stop.

### Step 2 — Nuke cache and restore production marketplace

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin marketplace add benkruger/flow
```

Then:

```bash
claude plugin marketplace update flow-marketplace
```

### Step 3 — Remove dev mode marker

Use Bash to remove the marker:

```bash
rm .flow-states/.dev-mode
```

### Step 4 — Report

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW QA — Dev mode stopped
============================================
```
````

## No flag (bare `/flow-qa`)

Check if `.flow-states/.dev-mode` exists using the Read tool.

- If dev mode is **active** → run the `--restart` flow above.
- If dev mode is **not active** → run the `--start` flow above.
