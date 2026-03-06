---
name: qa
description: "QA the FLOW plugin locally. Switch marketplace to local source, test in a live session, restore when done."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — requires the plugin to be installed.

## Usage

```text
/qa
/qa --start
/qa --stop
/qa --restart
```

- `/qa` — show dev mode status, then prompt for next action
- `/qa --start` — nuke cache, switch marketplace to local source, create `.dev-mode` marker
- `/qa --stop` — nuke cache, restore production marketplace, remove `.dev-mode` marker
- `/qa --restart` — nuke cache, re-register local source, refresh cache (must already be in dev mode)

## Flag: `--start`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it exists, print "Already in dev mode. Use `/qa --stop` to exit." and stop.

### Step 2 — Nuke cache and switch to local source

Get the project root from `git worktree list --porcelain` (first `worktree` line).

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
> Run `/qa --stop` when done.

## Flag: `--restart`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it does not exist, print "Not in dev mode. Use `/qa --start` first." and stop.

### Step 2 — Nuke cache and re-register local source

Get the project root from `git worktree list --porcelain` (first `worktree` line).

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

## No flag (bare `/qa`)

Check if `.flow-states/.dev-mode` exists using the Read tool.

If dev mode is **active**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW QA — Dev mode: ACTIVE
  Plugin cache is using local source.
============================================
```
````

Then use AskUserQuestion:

> "What would you like to do?"
>
> - **Restart QA** — refresh cache from local source
> - **Stop QA** — restore production marketplace

Then invoke the chosen flag (`--restart` or `--stop`).

If dev mode is **not active**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW QA — Dev mode: INACTIVE
  Plugin cache is using production source.
============================================
```
````

Then use AskUserQuestion:

> "Start QA dev mode?"
>
> - **Yes, start** — runs `/qa --start`
> - **No, cancel** — stop

Then invoke `--start` if chosen.
