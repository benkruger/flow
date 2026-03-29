# Flow-Start Arguments Are Opaque

When `/flow:flow-start` is invoked, ALL arguments after the command
are the feature name. Pass them through verbatim to `start-setup`.
Never interpret, summarize, discuss, or respond to the semantic
content of the arguments. The script handles sanitization and
truncation.

Even if arguments look like a question, a discussion, or a bug
report — they are the feature name and plan prompt.
