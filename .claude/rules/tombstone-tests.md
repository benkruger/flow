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

```python
def test_code_review_no_plugin_step():
    """Tombstone: removed in PR #587. Must not return."""
    content = _read_skill("flow-code-review")
    assert "code-review:code-review" not in content
```

## When to Add

Every intentional removal of a named feature, config axis,
external plugin dependency, or numbered step should leave a
tombstone test. The test docstring must reference the PR that
performed the removal so the intent is traceable.

## Naming Convention

`test_<scope>_no_<removed_thing>` — e.g.,
`test_code_review_no_plugin_step`,
`test_code_review_no_plugin_config_axis`.

When a plan task prescribes tombstone test names, verify the
prescribed name matches the `test_<scope>_no_<removed_thing>`
convention BEFORE finalizing the plan. Citing this rule file in
a plan task is not compliance — the prescribed name itself must
also follow the pattern. Catching the naming violation in Code
Review is too late; renaming forces a separate commit and adds
friction the Plan phase could have avoided.

## Self-Reference Avoidance

A tombstone assertion that searches a file for a forbidden string
trips itself if the assertion text contains that same string as a
literal substring. The test must assemble the needle at runtime
so the searched pattern is not a literal in the test's own source.

In Rust, use `concat!`:

```rust
#[test]
fn test_start_setup_no_wait_timeout_trait() {
    let source = include_str!("start_setup.rs");
    let needle = concat!("trait ", "WaitTimeout");
    assert!(!source.contains(needle), "...");
}
```

In Python, use `.format()` or string concatenation:

```python
def test_subprocess_runners_no_wait_with_output():
    needle = "wait_with_{suffix}".format(suffix="output")
    # ... assert needle not in source lines ...
```

Cross-file tombstones that iterate multiple source files must
also skip comment lines so historical references in explanatory
comments do not trip the assertion. In Rust, skip lines starting
with `//`. In Python contexts scanning Rust files, apply the
same filter.

## Error Messages

Tombstone assertion messages must describe the current state of the
codebase, not planned future work. Never reference a replacement
skill, feature, or mechanism that does not yet exist. If the
capability was removed without replacement, say so. If a
replacement is planned, reference the tracking issue number so the
claim is verifiable.
