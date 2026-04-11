---
name: flow-hygiene
description: "Audit instruction corpus health — CLAUDE.md, rules, and memory for staleness, misplacement, duplication, and contradictions."
---

# FLOW Hygiene

Audit the health of the project's instruction corpus. Reads CLAUDE.md,
`.claude/rules/*.md`, and auto-memory files, then checks for six types
of drift: stale references, orphaned content, unenforced claims,
misplaced content, duplicated constraints, and contradictions. Read-only
— reports findings but does not fix anything.

Complements `/flow:flow-doc-sync`, which compares code behavior against
documentation. This skill compares instruction surfaces against each
other and against the codebase structure they reference.

## Usage

```text
/flow:flow-hygiene
```

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v1.1.0 — flow:flow-hygiene — STARTING
──────────────────────────────────────────────────
```
````

## Finding Taxonomy

Six finding types, each with a severity tag:

| Tag | Severity | Meaning |
|-----|----------|---------|
| `[STALE]` | High | A file path, function name, test name, or command referenced in an instruction surface no longer exists in the codebase |
| `[ORPHANED]` | Medium | A rule file or CLAUDE.md section describes a feature, hook, or pattern that no longer exists — the content itself is orphaned, not just a single reference |
| `[UNENFORCED]` | Medium | An instruction claims enforcement ("CI enforces via X", "hook blocks Y") but the named enforcer does not exist or does not check the claimed condition |
| `[MISPLACED]` | Low | Content is stored in the wrong persistence layer per the routing decision tree — e.g., an imperative constraint in CLAUDE.md instead of a rule, or a project fact in memory instead of CLAUDE.md |
| `[DUPLICATE]` | Low | The same constraint or fact appears in multiple surfaces with identical or near-identical wording — creates update burden when the constraint changes |
| `[CONTRADICTION]` | High | Two surfaces prescribe opposite behavior for the same situation — one says "always do X", another says "never do X" |

## Steps

### Step 1 — Discover surfaces

Identify and read all instruction surfaces in the project.

Use the Glob tool to find:

- `CLAUDE.md` in the current working directory
- `.claude/rules/*.md` in the current working directory

Read each discovered file using the Read tool. For each file, note:

- File path
- All file paths, function names, test names, and commands mentioned in backticks
- All enforcement claims ("CI enforces", "hook blocks", "test validates")
- All behavioral constraints (imperatives: "never", "always", "must", "do not")
- All architectural facts (descriptions of how things work)

For auto-memory: read the memory index (`MEMORY.md`) from conversation
context if it is loaded. If individual memory files are referenced in the
index, read them using the Read tool. Memory files live at
`~/.claude/projects/*/memory/` and are user-scoped — they may not exist
for every project.

### Step 2 — Structural verification

Verify that references in instruction surfaces point to things that
exist. This is a mechanical pass — each check is a Grep or Glob call.

**File path references.** For each file path mentioned in backticks in
any surface (e.g., `src/utils.rs`, `tests/structural.rs`,
`hooks/hooks.json`), use the Glob tool to verify the file exists in the
current working directory. Tag missing files as `[STALE]`.

**Function and test name references.** For each function name or test
name mentioned in backticks (e.g., `utility_skills()`,
`test_agent_frontmatter_only_supported_keys`), use the Grep tool to
verify it exists as a definition in the codebase. Tag missing
definitions as `[STALE]`.

**Command references.** For each `bin/flow` subcommand mentioned (e.g.,
`bin/flow check-phase`, `bin/flow tombstone-audit`), use the Grep tool
to verify the subcommand is registered in the dispatcher or source. Tag
missing commands as `[STALE]`.

**Enforcement claims.** For each claim that names a specific enforcer:

- "CI enforces via `test_name`" — Grep for the test function definition
- "hook blocks" or "hook validates" — Grep for the hook name in `hooks/hooks.json`
- "`tests/file.rs` enforces" — verify the test file exists

If the named enforcer does not exist, tag as `[UNENFORCED]`. If the
enforcer exists but does not check the claimed condition (requires
reading the test or hook body), tag as `[UNENFORCED]` with a note
explaining the gap.

**Orphan detection.** After all reference checks, identify rule files
or CLAUDE.md sections where the majority of references are stale. If a
rule file's primary subject (the feature, hook, or pattern it describes)
no longer exists, tag the entire file as `[ORPHANED]`.

### Step 3 — Classification audit

Apply the persistence routing decision tree to each content block.

The decision tree (from `.claude/rules/persistence-routing.md`):

1. Is it specific to this user? → Memory
2. Is it a behavioral constraint? (imperative guardrail) → Rule
3. Is it project knowledge? → CLAUDE.md
4. Can you derive it by reading code? → Do not store anywhere

**CLAUDE.md audit.** Read each section of CLAUDE.md. For each content
block, apply the imperative test: "Can you phrase this as an imperative
('do X', 'never Y', 'when X do Y')?" If yes, it belongs in a rule
file, not CLAUDE.md. Tag as `[MISPLACED]` with a recommendation to
move it to `.claude/rules/<topic>.md`.

Exclude sections that are genuinely project knowledge even if they
contain imperative-sounding language (e.g., "Phase gates prevent
skipping ahead" is a description, not an instruction).

**Memory audit.** For each memory file, check whether its content
duplicates an existing rule or CLAUDE.md section. If the same
constraint appears in both memory and a rule file, the memory entry is
redundant. Tag as `[MISPLACED]` with a recommendation to remove the
memory entry (rules are the authoritative source).

Also check whether memory files contain architecture facts, code
patterns, or file paths — these should be derivable from the code
and do not belong in memory. Tag as `[MISPLACED]`.

### Step 4 — Cross-reference audit

Compare all surfaces pairwise for duplicates and contradictions.

**Duplicate detection.** For each behavioral constraint found in
Step 1, search all other surfaces for the same constraint expressed
in similar words. Exact duplicates and near-duplicates (same meaning,
different wording) both count. Tag as `[DUPLICATE]` with the two
source locations.

Focus on constraints that would need to be updated in multiple places
if the rule changed — identical constraints in CLAUDE.md and a rule
file, or the same rule stated in two different rule files.

**Contradiction detection.** For each pair of constraints, check
whether they prescribe opposite behavior. Common patterns:

- "Always do X" in one surface vs "Never do X" in another
- "Use A for this" vs "Use B for this" where A and B are mutually exclusive
- Conflicting scope — one surface says a rule applies to all phases, another limits it to specific phases

Tag as `[CONTRADICTION]` with both source locations and the specific
conflict.

### Step 5 — Report

Produce the findings report inline in the response.

**Summary line.** Start with a one-line summary:

> **Hygiene: N findings (X stale, Y orphaned, Z unenforced, W misplaced, V duplicate, U contradiction)**

If no findings, output:

> **Hygiene: Clean — no instruction corpus drift detected.**

**Findings by source file.** Group findings by the source file where
the problematic content lives. For each file with findings, show the
file path as a heading, then each finding:

```text
## <source_file_path>

**[STALE]** <description>
- References: <what it references>
- Status: <not found / renamed to X / removed>

**[UNENFORCED]** <description>
- Claims: <what it claims is enforced>
- Enforcer: <named test/hook>
- Status: <enforcer not found / enforcer does not check this>

**[MISPLACED]** <description>
- Content: <summary of the content>
- Current location: <where it is>
- Recommended location: <where it should be>

**[DUPLICATE]** <description>
- Location 1: <file:line>
- Location 2: <file:line>

**[CONTRADICTION]** <description>
- Surface 1: <file> says <X>
- Surface 2: <file> says <Y>
```

**Orphaned files.** List `[ORPHANED]` findings separately at the end
under an "## Orphaned" heading.

After the report, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v1.1.0 — flow:flow-hygiene — COMPLETE
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
- Focus on structural accuracy and classification correctness, not style
