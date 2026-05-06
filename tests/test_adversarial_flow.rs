//! Adversarial probe consumed during Phase 4 Code Review. The agent's
//! tests revealed real bugs (helper/hook divergence, first-occurrence
//! semantics) that were fixed in Step 4. The proper regression
//! guards for those behaviors live in `tests/flow_paths.rs` and
//! `tests/hooks/validate_worktree_paths.rs`. Worktree removal at
//! Phase 6 Complete disposes of this file as a side effect of
//! `git worktree remove`.
