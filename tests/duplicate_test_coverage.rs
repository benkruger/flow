//! Intentionally empty — the duplicate-test-coverage scanner's
//! regression path is fully covered by the Plan-phase gate
//! (`bin/flow plan-check` at three callsites: standard,
//! extracted, resumed) and the inline unit/integration tests in
//! `src/duplicate_test_coverage.rs` and `src/plan_check.rs`.
//!
//! An initial version of this file scanned the committed prose
//! corpus (`CLAUDE.md`, `.claude/rules/*.md`, `skills/**/SKILL.md`,
//! `.claude/skills/**/SKILL.md`) for backtick-quoted identifiers
//! that normalize to an existing test name. That scanner produced
//! 18+ false positives on the first run — every legitimate
//! educational citation in the rule files
//! (e.g. `test_agent_frontmatter_only_supported_keys` in CLAUDE.md,
//! `production_ci_decider_tree_changed_returns_not_skipped` in
//! `.claude/rules/extract-helper-refactor.md`) fired. Per
//! `.claude/rules/tests-guard-real-regressions.md` "Forbidden
//! patterns: Duplicate guards for a property already covered by an
//! existing plan-check scanner," the corpus check adds no
//! protection on top of the Plan-phase gate already shipped — a
//! plan that names an existing test is caught at plan-check time
//! regardless of whether the name was copied from a committed
//! prose file. Per `.claude/rules/scope-enumeration.md`
//! "False-positive sweep before expanding the vocabulary" (count
//! ≥ 5 → revert), the file remains here as a marker so future
//! sessions don't re-derive this conclusion.
