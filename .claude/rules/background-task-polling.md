# Background Task Polling

When a Bash tool invocation runs in the background
(`run_in_background: true`), the harness sends a `task-notification`
system-reminder when the subprocess completes. Wait for that
notification — never poll the output file via repeated `Read` tool
calls on the same path.

## Why

Polling defeats the harness's notification mechanism. Each Read
consumes turn budget without producing decision-relevant
information: the output file is streaming the subprocess's
stdout/stderr to disk as the subprocess runs, and reading it
mid-stream gives a partial snapshot that the next read replaces.
The notification is the single signal that the subprocess has
actually finished — only at that point is the output file complete
and stable for analysis.

## The Rule

For every background subprocess started via Bash with
`run_in_background: true`:

1. **Receive the notification.** The harness emits a
   `<task-notification>` system-reminder with the task id, exit
   status, and output-file path when the subprocess exits.
2. **Read the output file once, after the notification.** Use the
   Read tool a single time once the notification has arrived; the
   file is now complete.
3. **Do not Read the output file before the notification.**
   Repeated reads of the same path while the subprocess is still
   running is polling. Each read returns more dots/lines than the
   last but provides no decision-relevant information.

If you have other useful work to do while the subprocess runs, do
that work. If you have nothing else to do, output nothing — the
notification will arrive on its own.

## What This Is Not

This is not a prohibition on reading subprocess output. The Read
tool is the correct way to retrieve the output once the subprocess
has finished. The rule prohibits the polling pattern: reading the
same file multiple times to check "did it finish yet?".

## Cross-References

- `.claude/rules/ci-is-a-gate.md` — `bin/flow` subcommands must
  never run in the background. The polling case here is mostly
  relevant to agent subprocesses, broader CI runs that the harness
  auto-backgrounded due to long timeout, and similar long-running
  auxiliary tasks where the harness explicitly returned a
  background task id.
