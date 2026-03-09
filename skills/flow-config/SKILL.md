---
name: flow-config
description: "Display the current FLOW configuration from .flow.json — version, framework, and per-skill autonomy settings."
model: haiku
---

# FLOW Config — Display Configuration

## Usage

```text
/flow:flow-config
```

Display-only skill. Reads `.flow.json` from the project root and shows the current configuration.

## Steps

### Step 1 — Read config

Use the Glob tool to check for `.flow.json` at the project root.

If `.flow.json` does not exist, tell the user:

> "No `.flow.json` found. Run `/flow:flow-prime` to configure this project."

Stop here.

If `.flow.json` exists, read it with the Read tool.

### Step 2 — Display config

Output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v<version> — Config
============================================
  Framework: <framework>
============================================
```
````

Then display the skills configuration as a markdown table:

```text
| Skill     | Commit | Continue |
|-----------|--------|----------|
| start     | —      | manual   |
| code      | manual | manual   |
| simplify  | auto   | auto     |
| review    | auto   | auto     |
| security  | auto   | auto     |
| learning  | auto   | auto     |
| abort     | auto   | —        |
| cleanup   | auto   | —        |
```

Use the actual values from `.flow.json`. The table above is just an example.

**Column rules:**

- **Phase skills with both axes** (code, simplify, review, security, learning): show both `commit` and `continue` values from the nested object
- **Phase skills with continue only** (start): show `—` for Commit, show the `continue` value
- **Utility skills** (abort, cleanup): show the single string value under Commit, show `—` for Continue

**Legacy format handling:** If `.flow.json` has the old single-value format (e.g., `"code": "manual"` instead of `{"commit": "manual", "continue": "manual"}`), display the single value in both columns for phase skills that should have two axes.

If `.flow.json` has no `skills` key, show "No skills configured — using built-in defaults" instead of the table.

Tell the user they can override any setting at invocation time with `--auto` or `--manual` flags.

## Hard Rules

- Display only — never modify `.flow.json`
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
