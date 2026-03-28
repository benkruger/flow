---
name: flow-changelog-audit
description: "Audit the Claude Code CHANGELOG.md for plugin-relevant changes. Builds FLOW's integration surface model, fetches new changelog entries, categorizes as Adopt/Remove/Adapt, and files issues for approved items."
---

# FLOW Changelog Audit

Audit the Claude Code CHANGELOG.md for changes relevant to the FLOW plugin.
Maintainer-only — runs in the FLOW source repo.

The skill builds a deep understanding of FLOW's integration surface before
reading the changelog, then assesses each entry against that model to
recognize opportunities that shallow keyword matching would miss.

## Announce

Print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.0.1 — changelog-audit — STARTING
──────────────────────────────────────────────────
```
````

## Assessment Categories

Every changelog entry relevant to FLOW falls into one of three categories:

**Adopt** — What can FLOW do now that it couldn't before? New platform
capabilities that unlock features or improve existing ones. Example:
`updatedInput` on PreToolUse enabling programmatic AskUserQuestion
interception.

**Remove** — What can FLOW stop doing because Claude Code does it natively
now? Code FLOW carries that the platform has made redundant. Removals are
often the highest value: less code, less maintenance, fewer failure modes.

**Adapt** — What should FLOW do differently because the platform changed?
A hook event that now fires at a different time, a permission syntax that
deprecated the old form, a frontmatter field that replaces a workaround.

**Not relevant** — After investigation, the entry does not apply to
FLOW's integration surface. One-line explanation of why.

## Step 0 — Build integration surface model

Before reading the changelog, build a mental model of how FLOW integrates
with Claude Code. Read each of the following files using the Read tool and
note what Claude Code APIs, features, and capabilities FLOW currently uses.

**Hook registrations.** Read `hooks/hooks.json`. For each hook event
(SessionStart, PreToolUse, PostToolUse, PostCompact, Stop, StopFailure),
note the matchers and which scripts handle them.

**Hook scripts.** Read each script referenced in `hooks/hooks.json`:

- `hooks/session-start.sh` — what Claude Code session events it handles
- `lib/validate-ci-bash.py` — what PreToolUse capabilities it uses
- `lib/validate-worktree-paths.py` — what PreToolUse capabilities it uses
- `lib/validate-ask-user.py` — what PreToolUse capabilities it uses (especially `updatedInput` and `permissionDecision`)
- `lib/clear-blocked.py` — what PostToolUse capabilities it uses
- `lib/post-compact.py` — what PostCompact capabilities it uses
- `lib/stop-continue.py` — what Stop hook capabilities it uses
- `lib/stop-failure.py` — what StopFailure hook capabilities it uses

**Skill frontmatter.** Read the YAML frontmatter block (between the
opening and closing `---` delimiters) of 3 plugin skills:
`skills/flow-start/SKILL.md`, `skills/flow-code/SKILL.md`, and
`skills/flow-issues/SKILL.md`. Note which frontmatter fields FLOW uses.

**Agent frontmatter.** Read the YAML frontmatter block of
`agents/ci-fixer.md`. Note which agent frontmatter fields FLOW uses
(name, description, tools, maxTurns, hooks).

**Permission surface.** Read `.claude/settings.json`. Note the permission
structure: `defaultMode`, `permissions.allow`, `permissions.deny`, `exclude`.

**Installation surface.** Read `lib/prime-setup.py`. Note what Claude Code
configuration surfaces it writes to during project setup.

After reading all files, summarize the integration surface model in your
response. This summary is the foundation for Step 4 assessments.

## Step 1 — Read stored version

Use the Read tool to read `config.json` at the repo root. Extract the
`claude_code_audited` field value — this is the last version FLOW was
audited against.

Display the stored version in your response.

## Step 2 — Fetch changelog

Use WebFetch to fetch the Claude Code changelog:

URL: `https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md`

Prompt: "Extract all version entries. For each version, return the version
number and the full entry text. Start from the most recent version and go
backwards. Return every entry."

From the result, identify all versions newer than the stored version from
Step 1. These are the entries to audit.

If no new versions exist since the stored version, print the Done banner
and stop.

Display the count of new versions found.

## Step 3 — Filter entries

Filter the new version entries for plugin-relevant keywords. An entry is
relevant if it contains any of these terms:

`hook`, `frontmatter`, `agent`, `skill`, `worktree`, `plugin`,
`PreToolUse`, `PostToolUse`, `PostCompact`, `Stop`, `SessionStart`,
`StopFailure`, `permission`, `sandbox`, `matcher`, `manifest`,
`userConfig`, `AskUserQuestion`, `updatedInput`, `permissionDecision`,
`maxTurns`, `bypassPermissions`, `defaultMode`, `exclude`, `deny`,
`MCP`, `subagent`, `sub-agent`

This keyword list should be expanded as new Claude Code surface areas
emerge. When in doubt, include the entry — false positives are filtered
in Step 4, but false negatives are missed opportunities.

Display the count of relevant entries found. If zero, print the Done
banner and stop.

## Step 4 — Deep assessment

For each relevant entry from Step 3, assess it against the integration
surface model from Step 0.

**The critical rule:** grep for the *specific new API surface* mentioned
in each changelog entry, not just the parent feature. "FLOW uses
PreToolUse" does not mean "FLOW uses every capability of PreToolUse."
If a changelog entry mentions a new parameter, return value, or behavior
on an existing hook, grep for that specific parameter name in the FLOW
codebase using the Grep tool.

For each entry, determine:

**Adopt** — The entry describes a capability that FLOW's hooks, skills,
or agents do not currently use. Grep confirms the specific new API
surface does not appear in the codebase. The capability would enable
something FLOW cannot do today or would improve an existing feature.

**Remove** — The entry describes something that Claude Code now handles
natively, making FLOW's manual implementation redundant. Grep confirms
FLOW currently implements this manually. The FLOW code could be deleted
or simplified.

**Adapt** — The entry changes the behavior or API of something FLOW
currently relies on. Grep confirms FLOW uses the affected API. FLOW
may need to change how it uses it to remain correct or optimal.

**Not relevant** — After grepping, the entry does not apply to FLOW's
integration surface. One-line explanation of why.

In your rationale, always name the specific FLOW file(s) affected and
explain the connection.

## Step 5 — Present categorized tables

Present the findings in your response as three markdown tables. Skip any
table where the category has zero entries.

**Adopt**

| # | Version | Entry | Rationale |
|---|---------|-------|-----------|
| 1 | v2.X.Y | Brief description of the change | Which FLOW files are affected and what the opportunity is |

**Remove**

| # | Version | Entry | Rationale |
|---|---------|-------|-----------|
| 2 | v2.X.Y | Brief description of the change | Which FLOW code becomes redundant and why |

**Adapt**

| # | Version | Entry | Rationale |
|---|---------|-------|-----------|
| 3 | v2.X.Y | Brief description of the change | Which FLOW code needs updating and how |

Number rows sequentially across all three tables (not restarting per table).

After the tables, list entries categorized as "Not relevant" with
one-line explanations.

## Step 6 — File issues

<HARD-GATE>
Do NOT file any issues without explicit user approval. Do NOT proceed
to Step 7 without completing this step. Do NOT take any action outside
this skill flow.

Present the Adopt, Remove, and Adapt items from Step 5 and ask the user
which items to file as issues. Use AskUserQuestion:

"Which items should I file as issues? List the numbers, or say 'all'
or 'none'."

Wait for the user's response before taking any action.
</HARD-GATE>

For each approved item, invoke `/flow:flow-create-issue` with a
description that includes the version, the changelog entry, the
assessment category (Adopt, Remove, or Adapt), and the rationale
from Step 5.

If the user says "none", skip to Step 7.

## Step 7 — Update version marker

<HARD-GATE>
Do NOT update `config.json` without explicit user confirmation. Do NOT
commit without explicit user confirmation. Do NOT take any action outside
this skill flow.

Substitute the actual latest version from Step 2 in the prompt below,
then use AskUserQuestion:

"Audit complete. Update config.json to mark version &lt;latest version&gt;
as audited and commit?"

Options:
- **Yes, update and commit**
- **No, leave the version marker unchanged**

Wait for the user's response before taking any action.
</HARD-GATE>

If approved, use the Edit tool to update the `claude_code_audited` value
in `config.json` to the latest version audited in Step 2.

Then invoke `/flow:flow-commit` to commit the change.

If denied, skip the update.

## Done

Print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.0.1 — changelog-audit — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Rules

- Read-only until Step 6 — never create, edit, or close issues before the HARD-GATE
- Never update config.json before the Step 7 HARD-GATE
- Always grep for the specific new API surface, not just the parent feature name
- Build the integration surface model fresh each run — never rely on cached knowledge
- When in doubt about relevance, include the entry — false positives cost one line in a table, false negatives cost missed opportunities
- The keyword list in Step 3 is maintained in this skill — expand it when new Claude Code surface areas emerge
- Never add Co-Authored-By trailers or attribution lines
