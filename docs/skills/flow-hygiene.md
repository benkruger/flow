---
title: /flow-hygiene
nav_order: 15
parent: Skills
---

# /flow-hygiene

**Phase:** Any

**Usage:**

```text
/flow-hygiene
```

Audit the health of the project's instruction corpus. Reads CLAUDE.md, `.claude/rules/*.md`, and auto-memory files, then checks for six types of drift. Read-only — reports findings but does not fix anything.

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

Each finding includes the source file, specific content, and an actionable recommendation.

---

## Gates

- Read-only — never fixes, edits, or commits anything
- No state file mutations — stateless utility skill
- Display-only — no AskUserQuestion prompts
