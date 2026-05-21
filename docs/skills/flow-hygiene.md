---
title: /flow-hygiene
nav_order: 19
parent: Skills
---

# /flow-hygiene

**Phase:** Any

**Usage:**

```text
/flow-hygiene
```

Audit the health of the project's instruction corpus. Reads CLAUDE.md, `.claude/rules/*.md`, and auto-memory files, then checks for eight types of drift. Read-only — reports findings but does not fix anything.

Complements `/flow-doc-sync`, which compares code behavior against documentation. This skill compares instruction surfaces against each other and against the codebase structure they reference.

---

## What It Does

1. Discovers all instruction surfaces (CLAUDE.md, `.claude/rules/*.md`, memory files) and reads their content
2. Verifies structural references — file paths, function names, test names, commands, and enforcement claims — checking each exists in the codebase
3. Audits content classification against the persistence routing decision tree (Memory vs Rule vs CLAUDE.md)
4. Cross-references all surfaces pairwise for duplicated constraints and contradictions
5. Produces an inline findings report with a summary line and per-file findings

---

## Finding Types

| Tag | Severity | Meaning |
|-----|----------|---------|
| `[STALE]` | High | A referenced file path, function, test, or command no longer exists |
| `[ORPHANED]` | Medium | A rule file or section describes a feature that no longer exists |
| `[UNENFORCED]` | Medium | An enforcement claim names an enforcer that does not exist or does not check the condition |
| `[MISPLACED]` | Low | Content is in the wrong persistence layer (e.g., imperative in CLAUDE.md instead of a rule) |
| `[DUPLICATE]` | Low | The same constraint appears in multiple surfaces |
| `[CONTRADICTION]` | High | Two surfaces prescribe opposite behavior |
| `[CLAUDE_MD_MANDATE]` | High | A project-local rule mandates that descriptive content live in CLAUDE.md — the mandate is itself the misclassification per `persistence-routing` "Cross-Surface Application" |
| `[SIZE_BUDGET]` | Medium | CLAUDE.md exceeds the configurable size budget (`claude_md_budget` in `.flow.json`) |

Each finding includes the source file, specific content, and an actionable recommendation.

### CLAUDE.md mandate scan

Project-local rules that mandate CLAUDE.md prose for descriptive content invert the obey-vs-describe routing — a rule that says "X must be documented in CLAUDE.md" describes how the system works rather than enforcing a behavior. The scan greps every `.claude/rules/*.md` file for four canonical paraphrased substrings (`treats X added without Y documented in CLAUDE.md`, `must be documented in CLAUDE.md`, `documentation home is CLAUDE.md`, `CLAUDE.md as the canonical destination`) and emits a `[CLAUDE_MD_MANDATE]` finding citing the rule file path and the matched line. Each finding's recommendation routes the mandated prose to a feature-specific `.claude/rules/<feature>.md` file plus a one-line CLAUDE.md index entry. Matches inside quoted-example fences or paragraphs explicitly naming the pattern as an anti-pattern are excluded by construction.

### CLAUDE.md size budget

CLAUDE.md is reserved for behavioral instructions and one-line pointer indexes. Descriptive prose should live in feature-specific rule files, not in CLAUDE.md itself, so a CLAUDE.md that grows past a configurable budget signals that recent additions failed the obey-vs-describe gate. The skill reads `.flow.json` from the project root and parses the optional `claude_md_budget` object — fields `chars` (default 12000) and `lines` (default 400) — then measures CLAUDE.md and emits a `[SIZE_BUDGET]` finding when either threshold is exceeded. The finding cites the measured value, the budget, the override path, and the recommended fix.

To override the defaults, add the following to `.flow.json` at the project root:

```json
{
  "claude_md_budget": {
    "chars": 18000,
    "lines": 600
  }
}
```

---

## Gates

- Read-only — never fixes, edits, or commits anything
- No state file mutations — stateless utility skill
- No sub-agents — all comparison runs inline
- Display-only — no AskUserQuestion prompts
