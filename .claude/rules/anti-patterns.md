# Anti-Patterns

## Inline Output

When a phase produces output the user needs to review (plan file,
DAG analysis, review findings), render the full content inline in
the response text. Never tell the user to "look at" a file path
or "take a look" at a location — the user cannot see file contents
unless they are rendered in the response. The Read tool output
appears in tool results, but users expect the content presented as
formatted text in the response itself.

## Fix Before Remove

When a feature is broken, the first response must be to fix it —
not to remove it. Proposing removal as the initial approach
discards the user's intent. Only propose removal after demonstrating
that the feature cannot be fixed or after the user explicitly asks
for it.

## Imprecise Rule-File Mechanism Descriptions

When a rule file's Enforcement section describes what a hook or
validator matches, use the precise mechanism — not hand-wavy
language. "Starting with X" implies prefix matching on the raw
string; "ending with X" implies suffix matching; "containing X"
implies substring matching; "first token is X" implies whitespace
tokenization. A mismatch between the rule prose and the code's
matching logic misleads future readers trying to predict hook
behavior without reading the source.

When a rule references project-specific mode names ("FLOW-enabled",
"Standalone mode", "FLOW phase"), either define the term
parenthetically on first use or cite the skill file that defines
it (e.g. "see `skills/flow-commit/SKILL.md` Round 2"). A rule
file must stand alone for a reader who has not yet memorized the
project vocabulary.

When a rule explains a non-obvious design choice (e.g. "the
suffix match is intentional"), the explanation should cover
*why* it matters — what fails without it — not just *what* the
mechanism does. A newcomer who only sees "what" cannot judge
whether the mechanism still serves its purpose after a refactor.
