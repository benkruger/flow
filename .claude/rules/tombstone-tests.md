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

## Assertion Strength

A tombstone test is only as strong as its assertion. A byte-
substring check against a single literal (e.g.
`content.contains("\"start-lock\"")`) looks airtight but is
trivially bypassable — a merge resolver or a future author can
re-introduce the forbidden behavior with any construct that
produces the same string at runtime without the literal ever
appearing in source.

The byte-substring assertion `content.contains("\"start-lock\"")`
fails to catch ALL of:

- `concat!("start-", "lock")` — macro-concatenated literal
- `format!("{a}-{b}", a = "start", b = "lock")` — runtime format
- `["start-", "lock"].join("")` — slice join
- `const PREFIX: &str = "start-"; const SUFFIX: &str = "lock";` —
  split constants assembled later
- `let mut s = String::from("start-"); s.push_str("lock");` —
  mutating accumulation
- `"start-".to_string() + "lock"` — `String` addition
- `"\x73tart-lock"` — hex-escaped prefix
- `.arg("start-").arg("lock")` — chained method calls that pass
  the two halves as separate arguments

PR #1166 proved all eight of these bypasses with failing adversarial
tests. The initial tombstone for that PR shipped with a byte-
substring check and had to be rewritten under Code Review pressure.
The rewrite scans the function body of each protected test for
`Command::new(FLOW_RS)` — a construct no bypass can hide. This
section documents the strength criteria so future tombstones ship
strong from the start.

### Two kinds of tombstone

A tombstone protects against resurrection of one of:

1. **A stable source literal.** The forbidden thing is a fixed
   string that appears in source — a CLI argument quoted with
   double quotes (`"start-lock"`), a function name that cannot be
   synthesized at runtime (e.g. `post_message`), a file path, a
   config key. A byte-substring check is acceptable AS LONG AS
   the literal cannot be constructed by any of the patterns above
   and still produce the same runtime effect.
2. **A structural construct.** The forbidden thing is a class of
   runtime behavior (spawning a subprocess, opening a network
   socket, calling a deprecated API) that can be expressed through
   many different source shapes. The assertion must target the
   construct itself, not a specific string.

When in doubt, assume #2. Most "don't reintroduce this subprocess
call" or "don't reintroduce this API" cases are structural, even
when the current source happens to express them with a specific
literal.

### Structural tombstones — function-body scan

For structural assertions, scan the body of the function the
tombstone protects and assert the forbidden construct is absent
from the body. Use the bounded-slice pattern from
`.claude/rules/testing-gotchas.md` "Subsection-Local Assertions
in Contract Tests":

```rust
#[test]
fn test_concurrency_no_subprocess_start_lock() {
    // Tombstone: removed in PR #1166. Scan each protected test's
    // body for Command::new(FLOW_RS) regardless of how args are
    // constructed.
    let content = fs::read_to_string("tests/concurrency.rs")
        .expect("file must exist");

    const FORBIDDEN: &str = "Command::new(FLOW_RS)";
    const PROTECTED_FNS: &[&str] =
        &["start_lock_serialization", "thundering_herd_zero_delay"];

    for fn_name in PROTECTED_FNS {
        let marker = format!("fn {}(", fn_name);
        let tail = content
            .split_once(&marker)
            .map(|(_, t)| t)
            .expect("protected fn must exist");
        let body = tail
            .split_once("#[test]")
            .map(|(b, _)| b)
            .unwrap_or(tail);
        assert!(
            !body.contains(FORBIDDEN),
            "tests/concurrency.rs::{} must not contain `{}`",
            fn_name,
            FORBIDDEN
        );
    }
}
```

The `split_once("#[test]")` bounds the assertion scope to the
function body. An `unwrap_or(tail)` fallback handles the case
where the protected function is the last `#[test]` in the file.
For protected functions in the middle of the file, the bound is
the next `#[test]` attribute.

### Literal tombstones — stability checklist

When using a byte-substring check, the plan must document WHY the
literal is stable. For each claimed literal, answer:

1. **Can it be assembled by `concat!`?** If yes, the byte check
   fails when a future author uses `concat!`.
2. **Can it be produced by `format!`?** If yes, the byte check
   fails under format-string reassembly.
3. **Can it be a constant declared at the top of the file and
   referenced by name?** If yes, the byte check fails when the
   name-reference replaces the inline literal.
4. **Can the construct be split into multiple `.arg()` calls or
   other method chains?** If yes, structural scanning is
   required; byte-substring is insufficient.

If any answer is "yes", use a structural (function-body scoped)
tombstone instead. If all answers are "no", document WHY in the
test's doc comment so the next maintainer sees the reasoning.

### Plan-phase responsibility

When a plan proposes a tombstone, the Tasks section must specify:

1. **Protection target.** Exact feature, construct, or literal
   being protected.
2. **Assertion kind.** Literal (byte-substring) or structural
   (function-body scoped).
3. **Stability argument.** If literal, the four-question checklist
   above. If structural, the boundary markers used for the
   bounded-slice pattern.
4. **Bypass list.** For literal assertions, name at least three
   plausible bypasses the author considered and rejected with
   reasoning. For structural assertions, name the function(s)
   being scanned.

A tombstone proposal without this documentation is a Plan-phase
gap. Code Review's adversarial agent will write failing tests
against the weak assertion; the cheaper catch is at Plan time.

## Consolidation

Tombstones live in `tests/` — standalone file-existence and
source-content assertions go in `tests/tombstones.rs`, topical
tombstones integral to a test domain stay in their domain's
`tests/<name>.rs` file. All tests live under `tests/` per
`.claude/rules/test-placement.md`; an inline tombstone inside a
`src/*.rs` file is prohibited.

If a tombstone needs to call a crate-internal function, convert it
to a source-content assertion — read the source file at runtime
with `std::fs::read_to_string` and assert the removed pattern does
not appear. Source-content assertions run from `tests/` and need
no privileged access to the crate's internals.

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
