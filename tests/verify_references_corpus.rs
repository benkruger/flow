//! Documented empty marker for `src/verify_references_scanner.rs`
//! corpus contract.
//!
//! Per `.claude/rules/tests-guard-real-regressions.md` "Corpus-scan
//! viability check," the verify-references scanner is intentionally
//! NOT covered by a corpus contract test over `CLAUDE.md`,
//! `.claude/rules/*.md`, `skills/**/SKILL.md`, and
//! `.claude/skills/**/SKILL.md`. The scanner triggers on backtick-
//! quoted identifiers (≥ 10 chars, snake_case) inside a `## Tasks`
//! section. Rule and skill prose corpora do not have `## Tasks`
//! sections, so the corpus is structurally clean by definition —
//! adding a corpus test would assert a property that is guaranteed
//! by the section-scoping check rather than catching a real
//! regression.
//!
//! The Plan-phase gate at `bin/flow plan-check` and both
//! `src/plan_extract.rs` callsites already enforce the rule on
//! every plan written to `.flow-states/<branch>/plan.md`. That is
//! the regression path this scanner protects against; corpus
//! enforcement adds no protection on top per the rule's "Forbidden
//! patterns: Duplicate guards" subsection.
//!
//! If a future rule file or SKILL.md adds a `## Tasks` section
//! that legitimately cites identifiers, this file should be
//! converted to a real corpus contract test (see
//! `tests/cli_output_contract_corpus.rs` for the pattern).
