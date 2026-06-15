---
name: flow-doc-sync
description: "Full codebase documentation accuracy review — reports drift between code behavior and documentation."
---

# FLOW Doc Sync

Full codebase documentation accuracy review. Compares behavioral sources
(skills, lib scripts, config files) against all documentation surfaces
(README, docs pages, inline references) and produces a severity-tagged
drift report. Read-only — reports drift but does not fix anything.

## Usage

```text
/flow:flow-doc-sync
```

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v2.7.0 — flow:flow-doc-sync — STARTING
──────────────────────────────────────────────────
```
````

## Steps

### Step 1 — Discover project structure

Read `CLAUDE.md` at the project root using the Read tool. Identify:

- **Key files** — the files listed as important to the project
- **Architecture sections** — how the project is structured
- **Conventions** — rules and patterns the project follows

Use the Glob tool to find all documentation surfaces:

- `README.md`
- `docs/**/*.md`
- `docs/**/*.html`
- `CLAUDE.md` (also a documentation surface — it describes the project)
- `.claude/rules/*.md`

Record the full list of documentation surface paths for Step 2.

### Step 2 — Read sources

Read all behavioral sources identified from CLAUDE.md — skill files,
lib scripts, config files, hook definitions, phase definitions. These
are the source of truth for what the project actually does.

Read all documentation surfaces found in Step 1. For each surface,
note what it claims about project behavior, commands, file paths,
architecture, and conventions.

Use the Read tool for each file. For large files, read the sections
that make behavioral claims (skip license headers, boilerplate, etc.).

### Step 3 — Cross-reference

Compare each documentation surface against the behavioral sources.
For every behavioral claim in a doc surface, verify it against the
actual source. Tag each finding:

- **`[STALE]`** — the doc describes behavior that has changed. The
  feature still exists but works differently than documented.
  Include: what the doc says, what the code actually does, and the
  source file with line reference.

- **`[MISSING]`** — behavior exists in the code but is not documented
  in any surface. A feature, command, config option, or convention
  that users or maintainers should know about but cannot discover
  from documentation alone.

- **`[OUTDATED]`** — the doc references something that no longer
  exists: a removed file, renamed command, deleted config option,
  or deprecated pattern. The reference itself is the problem.
  Include: what the doc references and evidence it no longer exists.

- **`[DUPLICATE]`** — a CLAUDE.md section's prose duplicates
  content derivable from schema files, source docstrings, or existing rule files
  (all three reachable-elsewhere sources). The section is a maintenance
  burden because the same fact lives in two places and drifts
  independently. The duplicated prose fails the obey-vs-describe
  gate per `.claude/rules/persistence-routing.md` "Cross-Surface
  Application" — descriptive content should not live in CLAUDE.md.
  Include: the CLAUDE.md section, the alternative destinations
  where the same identifiers already appear, and the recommended
  fix (see "Duplication detection" below).

Skip cosmetic differences (formatting, word choice) that do not
affect accuracy. Focus on factual claims: commands, file paths,
behavior descriptions, config options, step counts, and
architectural statements.

**Duplication detection.** For every paragraph in CLAUDE.md that
runs at least three sentences in a description-shape (descriptive
prose about how something works rather than a behavioral imperative
like "do X" or "never Y"), extract the identifiers wrapped in
backticks: table names, function names, helper signatures, file
paths, type names. For each identifier, search via the Grep tool
against the project's schema files, source files (`src/**`,
`tests/**`), and existing `.claude/rules/*.md` files. When three or
more identifiers in the same paragraph all appear elsewhere, emit a
`[DUPLICATE]` finding that names the paragraph location and the
alternative destinations.

Behavioral-imperative paragraphs are excluded by construction — the
description-shape filter rejects them before the identifier scan
runs.

Pointer-index entries are also excluded. A one-line cross-reference
to a rule file (e.g. "**Tombstone tests** — see
`.claude/rules/tombstone-tests.md`.") is CLAUDE.md's sanctioned
content type per `.claude/rules/persistence-routing.md` "What
CLAUDE.md Is For" — its identifiers naturally appear elsewhere
because pointing at them is the entry's whole purpose. The
three-sentence description-shape filter already rejects one-line
pointers, but when a pointer-index section is read as a block, do
not flag its constituent entries as `[DUPLICATE]`.

Suggested-fix prose for each `[DUPLICATE]` finding:
"move prose to a feature rule at `.claude/rules/<feature>.md`
and reduce the CLAUDE.md section to a one-line CLAUDE.md index entry
per `.claude/rules/persistence-routing.md` Cross-Surface Application."

### Step 4 — Report

Produce the drift report inline in the response.

**Summary line.** Start with a one-line summary:

> **Doc Sync: N findings (X stale, Y missing, Z outdated, W duplicate)**

If no findings, output:

> **Doc Sync: No drift detected — documentation is in sync with code.**

**Findings.** List each finding grouped by documentation surface file.
For each file with findings, show the file path as a heading, then
each finding:

```text
## <doc_surface_path>

**[STALE]** <description>
- Doc says: <what the doc claims>
- Code does: <what the code actually does>
- Source: <behavioral_source_path>:<line>

**[OUTDATED]** <description>
- Doc references: <what it references>
- Status: <removed in commit/PR, renamed to X, etc.>
```

**Missing features.** List `[MISSING]` findings separately at the end
under a "## Undocumented" heading, since they are not tied to a
specific doc surface.

After the report, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.7.0 — flow:flow-doc-sync — COMPLETE
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Hard Rules

- Read-only — never fix, edit, or commit anything
- No state file mutations — this is a stateless utility skill
- No AskUserQuestion — produce the report and finish
- No sub-agents — all comparison is inline
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools
- Focus on factual accuracy, not style or formatting preferences
