//! Adversarial probe path for Code Review Phase 4.
//!
//! Untracked (in `.git/info/exclude` per `EXCLUDE_ENTRIES` in
//! `src/prime_check.rs`); the adversarial agent writes its
//! one-shot probe assertions here during Code Review and the file
//! is reconciled per `.claude/rules/adversarial-probe-lifecycle.md`
//! after Step 4 fixes land. Worktree removal at Phase 6 disposes
//! of the file as a side effect; the explicit
//! `delete_adversarial_probe` step in `src/cleanup.rs` lands the
//! disposal in the JSON `steps` audit trail.
