---
title: /flow-doc-sync
nav_order: 14
parent: Skills
---

# /flow-doc-sync

**Phase:** Any

**Usage:**

```text
/flow-doc-sync
```

Full codebase documentation accuracy review. Compares behavioral sources (skills, lib scripts, config files) against all documentation surfaces (README, docs pages, CLAUDE.md, rules) and produces a severity-tagged drift report. Read-only — reports drift but does not fix anything.

---

## What It Does

1. Discovers project structure by reading CLAUDE.md and using Glob to find all documentation surfaces (README.md, docs/\*\*/\*.md, docs/\*\*/\*.html, .claude/rules/\*.md)
2. Reads all behavioral sources identified from CLAUDE.md and all documentation surfaces
3. Cross-references each doc surface against behavioral sources, tagging findings by severity
4. Produces an inline drift report with a summary line and per-file findings

---

## Severity Tags

| Tag | Meaning |
|-----|---------|
| `[STALE]` | Doc describes behavior that has changed — feature exists but works differently |
| `[MISSING]` | Behavior exists in code but is not documented in any surface |
| `[OUTDATED]` | Doc references something that no longer exists — removed file, renamed command |
| `[DUPLICATE]` | A CLAUDE.md section duplicates content derivable from schema files, source docstrings, or existing rule files |

Each finding includes doc-says/code-does pairs with source file references.

### `[DUPLICATE]` and the identifier-overlap heuristic

CLAUDE.md sections that re-state content already documented elsewhere create a maintenance burden — the same fact lives in two places and drifts independently. The duplicated prose also fails the obey-vs-describe gate per `persistence-routing` "Cross-Surface Application": descriptive content should live in feature-specific rule files, not in CLAUDE.md itself.

The skill scans every paragraph in CLAUDE.md that runs at least three sentences in description-shape (descriptive prose, not behavioral imperatives like "do X" or "never Y"), extracts the identifiers wrapped in backticks (table names, function names, helper signatures, file paths, type names), and greps each identifier against the project's schema files, source files (`src/**`, `tests/**`), and existing `.claude/rules/*.md` files. When three or more identifiers in the same paragraph all appear elsewhere, the skill emits a `[DUPLICATE]` finding citing the paragraph location and the alternative destinations. The recommendation is to move the prose to a feature rule at `.claude/rules/<feature>.md` and reduce the CLAUDE.md section to a one-line index entry. Behavioral-imperative paragraphs are excluded by construction.

---

## Gates

- Read-only — never fixes, edits, or commits anything
- No state file mutations — stateless utility skill
- Display-only — no AskUserQuestion prompts
