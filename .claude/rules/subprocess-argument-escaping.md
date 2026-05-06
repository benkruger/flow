# Subprocess Argument Escaping

When a value sourced from outside the process (state file, git
output, user config, parsed JSON, env var, CLI arg) is interpolated
into a string that another interpreter will parse — AppleScript via
`osascript`, shell via `bash -c`, SQL via a query string, regex,
JSON, etc. — the value MUST be escaped according to that
interpreter's literal-syntax rules before interpolation. Raw
`format!` interpolation is an injection vector.

## Why

State files live on disk and are writable by any process with
filesystem access — including a future extension or a hand edit
that produced a malformed value. Subprocess output is constrained
by the producer but not by FLOW. CLI args accept any string a shell
can pass.

A crafted value containing the target language's structural
characters (e.g., `"` in AppleScript) closes the literal early and
lets everything after run as code under the user's privileges.

## The Rule

For every interpolation of an externally sourced string into a
language-bearing string literal, run the value through an explicit
escape helper before `format!`. The escape helper:

1. **Names the target language** in its function name —
   `escape_applescript_string`, `escape_shell_arg`,
   `escape_sql_literal`, etc. Generic "sanitize" or "clean" helpers
   are a smell.
2. **Has a doc comment that names the structural characters** for
   that language. AppleScript's are `\` and `"`. Shell's are the
   full operator set plus quotes. SQL's depend on the dialect.
3. **Is exhaustively unit-tested** against:
   - the empty input,
   - input containing only safe chars,
   - input containing only the structural char(s),
   - input containing both safe and structural chars,
   - input that LOOKS safe but contains the escape char itself
     (e.g. backslash before quote in AppleScript).
4. **Is the ONLY path** by which external values reach the
   interpolation site. No shortcuts, no "this caller is safe."

## Reference Implementation

The canonical example is `escape_applescript_string` in
`src/tui.rs`, used by `build_iterm_activation_script`. It escapes
`\` and `"` (the only structural characters inside AppleScript
double-quoted literals) with a leading backslash. Adversarial tests
prove the injection substring `" then do shell script` cannot
appear unescaped in the output for a malicious input.

## Where This Applies

- **`osascript -e <script>`** — escape AppleScript string literals
  before interpolation.
- **`Command::new("sh").arg("-c").arg(<script>)`** — avoid shell
  interpolation entirely; pass arguments via `.arg()` instead. If
  shell interpolation is unavoidable, escape per shell-quoting
  rules.
- **`format!("SELECT * WHERE x = '{}'", val)`** — never. Use a
  parameterized query.
- **`format!("{{\"key\": \"{}\"}}", val)`** — never. Use
  `serde_json::to_string`.
- **Any external value reaching a regex pattern** — use
  `regex::escape`.
- **Any external value reaching a shell command via `bash -c`** —
  use a quoting helper or restructure to avoid `bash -c`.

## Plan-Phase Trigger

When a plan task proposes building an interpolated string from
external input, the plan's Risks section must enumerate:

1. The **target interpreter** the string will be parsed by
2. The **structural characters** that interpreter treats as syntax
3. The **escape function** the implementation will call before
   interpolation
4. The **adversarial test** that will prove injection is impossible

## How to Apply (Code Phase)

1. Write the escape helper FIRST. Test it before writing the
   interpolation site — TDD applies here even more strongly than
   usual because the security property is non-obvious.
2. Make the interpolation site call the escape helper with NO
   conditional bypass. "Trusted callers" don't exist over time.
3. Write at least one adversarial test that:
   - Constructs a value that would inject if interpolated raw
   - Calls the production interpolation function
   - Asserts the injected substring does NOT appear in the output
     in a position where the target interpreter would execute it

## How to Apply (Code Review Phase)

When the reviewer agent or adversarial agent finds an interpolation
of external input, verify:

1. The escape helper exists and is named after the target language
2. The helper has tests covering all categories above
3. The interpolation site uses the helper with no bypass path
4. The helper's `format!` (or equivalent) interpolates the
   ESCAPED value, not the raw input
