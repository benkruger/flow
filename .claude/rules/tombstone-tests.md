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
the request.

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

The four categories cover distinct surfaces — overlooking any of
them produces unprotected deletions. A removal plan must
enumerate which categories its deletions fall into and add a
tombstone for each:

- **Named feature.** A SKILL.md, public function, public
  struct, public enum variant, public trait, or any other
  exported identifier that other code can reach by name.
  Tombstone shape: file-existence check (for SKILL.md, source
  files) and/or source-content byte-substring check (for
  function names, struct names, enum variants).
- **Config axis.** An entry in any configuration surface that
  governs runtime behavior — `.claude/settings.json`
  allow/deny entries, `flow-phases.json` phase or
  back-transition entries, `.flow.json` schema fields,
  `hooks/hooks.json` hook registrations, version pins in
  `Cargo.toml`/`Gemfile.lock`/`package.json`, environment-
  variable defaults, feature-flag entries. Tombstone shape:
  source-content check that asserts the removed entry's exact
  string does NOT appear in the config file.
- **External plugin dependency.** A reference to another
  Claude Code plugin (`code-review:code-review`,
  `decompose:decompose`), a third-party MCP server, or any
  other named external integration the skill or hook used to
  invoke. Tombstone shape: scan the SKILL.md or hook script
  for the integration's invocation surface and assert it does
  not appear.
- **Numbered step.** A step in a phase skill (`### Step 7`)
  that was removed. Tombstone shape: scan the skill's content
  and assert the step header is absent. Pair with a check that
  the total step count in the skill's HARD-GATE matches the
  current count, so a future re-numbering catches drift.

Every category benefits from a stability argument per
"Literal tombstones — stability checklist" below.

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

### Two kinds of tombstone

A tombstone protects against resurrection of one of:

1. **A stable source literal.** The forbidden thing is a fixed
   string that appears in source — a CLI argument quoted with
   double quotes (`"start-lock"`), a function name that cannot be
   synthesized at runtime, a file path, a config key. A
   byte-substring check is acceptable AS LONG AS the literal
   cannot be constructed by any of the patterns above and still
   produce the same runtime effect.
2. **A structural construct.** The forbidden thing is a class of
   runtime behavior (spawning a subprocess, opening a network
   socket, calling a deprecated API) that can be expressed through
   many different source shapes. The assertion must target the
   construct itself, not a specific string.

When in doubt, assume #2. Most "don't reintroduce this subprocess
call" or "don't reintroduce this API" cases are structural, even
when the current source happens to express them with a specific
literal.

A separate dimension that always applies regardless of which
kind of tombstone the situation calls for: **file-resurrection
threats**. When the deletion target includes a source file (a
deleted `src/<name>.rs`, `tests/<name>.rs`, or
`.claude/skills/<name>/SKILL.md`), pair the byte-substring
tombstone with a file-existence tombstone for the same path.
Without the path-based pair, `#[path = "<name>.rs"] pub mod
<alias>;` resurrects the file under a renamed module and the
byte-substring tombstone never fires. The file-existence check
catches the file regardless of how it is imported.

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
5. **File-resurrection pair.** If the deletion target includes a
   source file, name the file-existence tombstone that pairs with
   the byte-substring tombstone, per "Two kinds of tombstone"
   above.

A tombstone proposal without this documentation is a Plan-phase
gap.

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
not appear.

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
