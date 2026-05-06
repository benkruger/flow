# External-Input Path Construction

When a state-derived string (state-file value, env var, parsed JSON
field, hand-edited config) flows into filesystem path construction,
the value must pass a positive validator BEFORE the path is built.
Same discipline for external file reads: every read of a
caller-controlled or state-derived path must enforce a documented
size cap so a corrupted or hostile input cannot cause unbounded I/O.

This rule is a specialization of
`.claude/rules/external-input-validation.md` for the specific case
of strings reaching `format!`, `Path::join`, `PathBuf::from`, or
`fs::*::open`. The parent rule covers panicking constructors; this
rule covers the silent-corruption surface where untrusted strings
become filesystem paths or file descriptors.

## Why

Two failure modes recur whenever state-derived strings reach the
filesystem:

1. **Path traversal / arbitrary file read.** A `session_id` or
   `transcript_path` value lives in `state.json` — a file that
   any process with filesystem access can hand-edit, that
   external tools can corrupt, and that interrupted writes can
   leave malformed. Interpolating that value into
   `format!("{}.txt", session_id)` or passing it to
   `fs::File::open(path)` lets a malicious or accidental string
   redirect a read to `../../etc/passwd`,
   `~/.config/gh/hosts.yml`, or any file the running user can
   read. The fail-open pattern (`.ok()?`) makes the redirect
   silent.
2. **Unbounded resource consumption.** A transcript file or
   session log can grow to hundreds of megabytes during a long
   autonomous flow. When a per-step capture re-reads the file
   at every state mutation, total I/O scales as O(producers ×
   file_size) and can OOM-kill the process mid-write,
   corrupting the state file. The module's "no panic, no
   block" promise is broken whenever the read is unbounded.

Both failure modes are silent. Production code paths return
`None` or default values when the read fails, masking the
attempted attack or the resource exhaustion. The defense must
happen BEFORE the read, in a positive validator.

## The Rule

For every state-derived or env-var-derived string that flows
into a filesystem path or file open:

1. **Validate before constructing.** Pass the value through a
   positive validator (`is_safe_<purpose>`) that rejects empty
   strings, traversal segments (`.`, `..`), path separators
   (`/`, `\`), NUL bytes, and any character outside the closed
   set the production code expects.
2. **Validate against a known-good prefix when possible.** When
   the value is a path itself (transcript path, log path,
   etc.), require it to be absolute AND rooted under a
   well-known directory (`<home>/.claude/projects/`,
   `<project_root>/.flow-states/`). Reject anything outside the
   prefix.
3. **Enforce a documented size cap on every external read.**
   `BufReader::new(file.take(BYTE_CAP))` is the canonical
   pattern. The cap lives as a module-level `const` with a doc
   comment naming the worst-case scenario it bounds.
4. **Document the validation contract in the function's doc
   comment.** Name the rejected character classes, the
   prefix-containment requirement, and the byte cap. Future
   sessions reading the function must see the contract without
   tracing callers.
5. **Validate env-var-derived paths as absolute.** When a
   module reads `$HOME` (or any env var) and joins it with a
   relative suffix, an unset or relative env var produces a
   cwd-relative path that resolves against the worktree root.
   In a hostile or misconfigured repo, that lets a committed
   `.claude/rate-limits.json` (or equivalent) be read as the
   user's data. Reject empty / non-absolute env-var values
   before joining.

## What Counts as State-Derived

A string is state-derived when its value comes from any of:

- A field in `.flow-states/<branch>/state.json` (or any other
  on-disk JSON the process reads at runtime)
- An environment variable (including `HOME`, `USER`, custom
  vars)
- Parsed JSON from a subprocess (`gh issue view --json ...`,
  `git log --format=%H ...`)
- A user-supplied CLI flag (`--branch`, `--issue`)
- A hand-editable config file (`.flow.json`, `.claude/settings.json`)

State-derived is NOT:

- A compile-time constant (`const FOO: &str = "..."`)
- A value derived from a guaranteed-valid source already
  validated upstream (e.g., a branch name produced by
  `branch_name()` at flow-start, then read back from state in
  the same process)

When in doubt, treat as state-derived and validate.

## Reference Implementation

`src/window_snapshot.rs` is the canonical example:

- `is_safe_session_id(s: &str) -> bool` — accepts only ASCII
  alphanumeric plus `-` and `_`; rejects empty, `.`, `..`, and
  any other character.
- `is_safe_transcript_path(path: &Path, home: &Path) -> bool`
  — requires absolute, NUL-free, rooted under
  `<home>/.claude/projects/`.
- `read_rate_limits(home: &Path)` — early-returns when `home`
  is empty or non-absolute, preventing cwd-relative resolution
  of `.claude/rate-limits.json`.
- `read_transcript(path: &Path)` — caps reads at
  `TRANSCRIPT_BYTE_CAP = 50 MB` via
  `BufReader::new(file.take(TRANSCRIPT_BYTE_CAP))`. The
  constant carries a doc comment naming the long-autonomous-flow
  scenario it bounds.

## Plan-Phase Trigger

When a plan task proposes a new function that reads a
state-derived or env-derived string and uses it in filesystem
path construction or `fs::*::open`, the plan's Risks section
must enumerate:

1. **Source.** Where does the string enter the process? (state
   field, env var, JSON parse, CLI flag, etc.)
2. **Sink.** What construction does the string flow into?
   (`format!`, `Path::join`, `PathBuf::from`, `fs::File::open`,
   etc.)
3. **Validator.** Which `is_safe_<purpose>` function (or
   equivalent positive validator) gates the construction?
4. **Cap.** For every file read, the byte cap that bounds I/O.

A plan that proposes a new external-input read without naming
all four is incomplete.

## How to Apply

**Code phase.** When implementing a function that reads a
state-derived string and uses it in path construction:

1. Write the validator helper FIRST — its doc comment names
   the rejection classes and the prefix-containment
   requirement.
2. Call the validator at the boundary where the untrusted value
   enters the function — typically a public parameter or a JSON
   field extraction. `.filter(is_safe_*)` on `Option<&str>`
   extracted from JSON is the canonical pattern.
3. For file reads, declare the byte cap as a module-level
   `const` with a doc comment, then wrap the reader in
   `BufReader::new(file.take(THAT_CAP))`.
4. Test every rejection class — empty, `.`, `..`, traversal,
   NUL byte, prefix-escape, oversized read. The tests live
   alongside the validator, not at the consumer.

**Code Review phase.** The reviewer agent and adversarial
agent check every new state-derived path-construction site for
the validator. Findings tagged "path traversal via X" or
"arbitrary file read via Y" are Real findings that get fixed
in the same PR per `.claude/rules/code-review-scope.md`.

## Cross-References

- `.claude/rules/external-input-validation.md` — the parent
  rule covering panicking constructors.
- `.claude/rules/external-input-audit-gate.md` — the
  Plan-phase mechanical gate. Its trigger vocabulary today
  catches `panic!` / `assert!` tightenings; the patterns here
  (untrusted-string-into-`format!`-into-`Path`) are not yet in
  that vocabulary, so the gate cannot mechanically catch this
  class. Future work: extend the gate's trigger vocabulary to
  cover `PathBuf::from(<state-value>)` and
  `fs::File::open(<state-value>)` patterns.
- `.claude/rules/security-gates.md` "Normalize Before
  Comparing" — the sibling rule for string comparisons in
  gates.
- `.claude/rules/subprocess-argument-escaping.md` — the
  sibling rule for state-derived strings flowing into shell /
  AppleScript / SQL interpolation.
