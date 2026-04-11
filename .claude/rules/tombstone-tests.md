# Tombstone Tests

When a feature, config axis, step, or external dependency is
intentionally removed, add a test that asserts the removed
identifier does NOT appear in the source file. This converts
deletion intent from absence (invisible to three-way merges)
into presence (fails CI on resurrection).

## Why

Deletions leave no positive evidence. Removing tests that assert
"X exists" does not add tests asserting "X must not exist." When
a feature branch forked before the deletion merges main, the
merge resolver sees both sides and may keep everything — the
deleted content returns alongside its matching tests, CI passes,
and the deletion is silently undone.

A tombstone test is a negative assertion that survives on main
and catches this: if a merge conflict resolution re-introduces
the deleted content, the tombstone test fails immediately.

## Pattern

The tombstone comment must follow the format `Tombstone:` (case-
sensitive) followed by any text, then `PR #` and digits. The
`tombstone-audit` subcommand uses the regex `Tombstone:.*?PR #(\d+)`
to extract PR numbers — comments that don't match this pattern are
invisible to the audit.

**Only `PR #<number>` is recognized.** Alternatives like
`issue #<number>`, `commit <sha>`, `for ticket <N>`, or `per PR
<N>` are invisible to the audit and will never be counted as stale
no matter how old the underlying PR is. Use `PR #<number>` exactly,
even if the conceptual "source" of the removal was an issue — cite
the merge PR that performed the removal, not the issue that filed
the request. If the removal landed outside a PR (e.g. a direct
push, which should never happen in this repo but can in others),
the tombstone is inauditable and should be accompanied by a doc
comment explaining why.

```rust
#[test]
fn test_code_review_no_plugin_step() {
    // Tombstone: removed in PR #587. Must not return.
    let content = common::read_skill("flow-code-review");
    assert!(!content.contains("code-review:code-review"));
}
```

## When to Add

Every intentional removal of a named feature, config axis,
external plugin dependency, or numbered step should leave a
tombstone test. The test comment must reference the PR that
performed the removal so the intent is traceable.

## Naming Convention

`test_<scope>_no_<removed_thing>` — e.g.,
`test_code_review_no_plugin_step`,
`test_code_review_no_plugin_config_axis`.

## Error Messages

Tombstone assertion messages must describe the current state of the
codebase, not planned future work. Never reference a replacement
skill, feature, or mechanism that does not yet exist. If the
capability was removed without replacement, say so. If a
replacement is planned, reference the tracking issue number so the
claim is verifiable.

## Consolidation

When removing a feature tested inline in a `src/*.rs` file, put
the tombstone in `tests/tombstones.rs` rather than adding an
inline `#[cfg(test)]` tombstone to the source file. This keeps
source files focused on production code and makes the tombstone
inventory discoverable.

If the tombstone needs to call crate-internal functions, convert
it to a source-content assertion instead — read the source file
at runtime with `std::fs::read_to_string` and assert the removed
pattern does not appear.

## Lifecycle

Tombstones have two halves: creation and removal.

**Creation.** Add a tombstone when removing a feature. Standalone
tombstones (file-existence, source-content checks) go in
`tests/tombstones.rs`. Topical tombstones that are integral to a
test domain (skill_contracts, structural, dispatcher) stay in
their respective test files.

**Removal.** The `bin/flow tombstone-audit` subcommand scans all
`tests/*.rs` files for PR references, queries GitHub for merge
dates, and classifies each as stale or current. The command
requires the `gh` CLI tool and authenticated GitHub access. If
network access or authentication fails, the audit skips gracefully
and no stale tombstones are removed.

A tombstone is stale when the PR that removed the feature was
merged before the oldest open PR was created. For example, if
PR #839 merged on 2024-01-15 and the oldest open PR was created
on 2024-06-01, then tombstone PR #839 is stale — no branch could
have been created before 2024-01-15 and still be open today, so
the deleted code cannot be resurrected via merge conflict.

Code Review Step 1 runs the audit automatically; Step 4 removes
stale tombstones.
