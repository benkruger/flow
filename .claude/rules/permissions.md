# Permission Patterns

## Specificity Over Breadth

Use the narrowest pattern that serves the consumer. When the
consumer needs a known file extension, use that extension — never
replace it with a wildcard.

- `Read(//tmp/*.md)` — correct when the consumer reads markdown
- `Read(//tmp/*)` — too broad; covers every file type in `/tmp/`

Directory-level wildcards are acceptable only when every file in
the directory is a valid target. `Read(~/.claude/rules/*)` is fine
because all files in that directory are rules.

## Consumer Traceability

Every allow-list pattern must have a known consumer — a specific
skill, plugin, hook, or tool that needs the permission. If you
cannot name the consumer, do not add the pattern.

Before proposing a new pattern, answer: "Which skill or tool
invokes this command or reads this path?" If the answer is vague
("something might need it"), the pattern is speculative and should
not be added.

## Adding Patterns

When adding a new allow-list pattern, name the consumer in the
commit message or PR description so the audit trail is preserved.

Example commit message:

```text
Add Read(//tmp/*.diff) permission for code-review plugin
```

This makes the allow list auditable — any pattern can be traced
back to why it was added and what breaks if it is removed.
