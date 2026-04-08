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
