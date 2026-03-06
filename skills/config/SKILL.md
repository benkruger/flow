---
name: config
description: "Display the current FLOW configuration from .flow.json — version, framework, and per-skill autonomy settings."
model: haiku
---

# FLOW Config — Display Configuration

## Usage

```text
/flow:config
```

Display-only skill. Reads `.flow.json` from the project root and shows the current configuration.

## Steps

### Step 1 — Read config

Use the Glob tool to check for `.flow.json` at the project root.

If `.flow.json` does not exist, tell the user:

> "No `.flow.json` found. Run `/flow:init` to configure this project."

Stop here.

If `.flow.json` exists, read it with the Read tool.

### Step 2 — Display config

Print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````text
```
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
| security  | —      | auto     |
| reflect   | auto   | auto     |
| commit    | manual | —        |
| abort     | auto   | —        |
| cleanup   | auto   | —        |
```

Use the actual values from `.flow.json`. The table above is just an example.

**Column rules:**

- **Phase skills with both axes** (code, simplify, review, reflect): show both `commit` and `continue` values from the nested object
- **Phase skills with continue only** (start, security): show `—` for Commit, show the `continue` value
- **Utility skills** (commit, abort, cleanup): show the single string value under Commit, show `—` for Continue

**Legacy format handling:** If `.flow.json` has the old single-value format (e.g., `"code": "manual"` instead of `{"commit": "manual", "continue": "manual"}`), display the single value in both columns for phase skills that should have two axes.

If `.flow.json` has no `skills` key, show "No skills configured — using built-in defaults" instead of the table.

Tell the user they can override any setting at invocation time with `--auto` or `--manual` flags.

## Hard Rules

- Display only — never modify `.flow.json`
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
