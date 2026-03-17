---
name: flow-qa
description: "QA the FLOW plugin locally. Uninstall marketplace plugin for local testing, reinstall when done."
---

# FLOW QA

Test the FLOW plugin locally before releasing. Maintainer-only — requires the plugin to be installed.

## Usage

```text
/flow-qa
/flow-qa --start
/flow-qa --stop
```

- `/flow-qa` — show current mode (dev or marketplace)
- `/flow-qa --start` — uninstall marketplace plugin (if installed), nuke cache, create `.dev-mode` marker, tell user to use `--plugin-dir`
- `/flow-qa --stop` — nuke cache, reinstall marketplace plugin, remove `.dev-mode` marker

## Bare `/flow-qa` (no flags)

### Step 1 — Check dev mode marker

Use the Read tool to check if `.flow-states/.dev-mode` exists.

### Step 2 — Report

If `.flow-states/.dev-mode` exists, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE (local)
──────────────────────────────────────────────────
```
````

If `.flow-states/.dev-mode` does not exist, print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — MARKETPLACE (remote)
──────────────────────────────────────────────────
```
````

## Flag: `--start`

### Step 1 — Check if marketplace plugin is installed

Run:

```bash
claude plugin list
```

If the output contains `flow@flow-marketplace`, run:

```bash
claude plugin uninstall flow@flow-marketplace
```

If the output does not contain `flow@flow-marketplace`, print "Marketplace plugin not installed, nothing to uninstall." and continue.

### Step 2 — Nuke cache

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

### Step 3 — Create dev mode marker

Use the Write tool to create `.flow-states/.dev-mode` with the content `active`.

### Step 4 — Announce

Print inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW QA — DEV MODE ACTIVE
──────────────────────────────────────────────────
```
````

Then print:

> To test local source, start Claude Code with:
>
> `claude --plugin-dir=$HOME/code/flow`
>
> Run `/flow-qa --stop` when done to reinstall the marketplace plugin.

Then tell the user:

> Run `/reload-plugins` now to update the skill list for this session.

## Flag: `--stop`

### Step 1 — Check dev mode

Use the Read tool to check if `.flow-states/.dev-mode` exists.

If it does not exist, print "Not in dev mode. Nothing to stop." and stop.

### Step 2 — Nuke cache and reinstall marketplace plugin

Run:

```bash
rm -rf ~/.claude/plugins/cache/flow-marketplace
```

Then:

```bash
claude plugin install flow@flow-marketplace
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
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW QA — Dev mode stopped
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

Then tell the user:

> Run `/reload-plugins` now to update the skill list for this session.
