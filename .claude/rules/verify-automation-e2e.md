# Verify Automation End-to-End

Never ship an automation feature without running the full execution
path first. If a feature claims to "run X automatically", actually
run it before marking it complete.

If the feature cannot be tested in the current context (bootstrapping
problem), flag that explicitly rather than shipping and hoping.

## What Counts as "Flag Explicitly"

The bootstrapping carve-out above is narrow. It applies when the
test would require an environment the current session structurally
cannot provide — not when the test is merely inconvenient or
slow. The two canonical bootstrapping scenarios:

1. **Test would re-enter the active workflow.** A FLOW lifecycle
   feature cannot be E2E-tested from inside an active FLOW
   session, because `/flow-qa --start` bootstraps a NEW lifecycle
   in a target QA repo and would conflict with the active flow's
   state file, lock, and worktree.
2. **Test would require operator approval the agent cannot
   provide.** A feature that requires interactive credential
   entry, hardware access, or a manual UI confirmation cannot
   run autonomously inside the FLOW Code phase.

When the carve-out applies, the deferral must satisfy ALL of:

1. **Logged via `bin/flow log`** with explicit text describing
   what was deferred, why the bootstrapping prevented in-session
   testing, and what command the user must run after merge to
   verify (e.g., `/flow-qa --start` in a fresh Claude Code
   session). The log entry serves three readers: Code Review's
   reviewer agent (which checks deferral discipline), the Learn
   analyst (which audits whether the deferral was reasonable),
   and the user (post-merge action).
2. **Documented in the commit message body.** The same
   explanation appears in the commit message that lands the
   feature, so a future session reading `git log` sees the
   deferral without consulting the state log file.
3. **NOT used for security-sensitive features.** Authentication,
   authorization, encryption, secret handling, sandbox escapes,
   and external-input validation features cannot defer their
   E2E test. If the bootstrapping problem prevents in-session
   testing of a security feature, the design must change so the
   security path can be exercised inline.

## What Does NOT Count as "Flag Explicitly"

- A TODO comment in the code
- A note in the PR description with no log entry
- A verbal commitment without a written record
- A deferral for a feature that COULD be tested in the current
  session but the agent decided to skip for time reasons

The cost of the explicit-deferral discipline is low — one
`bin/flow log` call plus a few sentences in the commit message.
The cost of letting an untested feature ship without record is
that the next session has no signal that verification is owed.

## How to Apply

**Code phase.** When you encounter an automation feature whose
E2E test would require bootstrapping the very environment the
current session is using:

1. Stop and verify the bootstrapping problem is real — name
   the specific environment conflict (state file, lock,
   worktree, lifecycle re-entry).
2. If real, log the deferral with `bin/flow log` per the
   format above.
3. Include the deferral rationale in the commit message body.
4. If the feature is security-sensitive (see list above),
   redesign instead of deferring.

**Code Review phase.** The reviewer agent checks that any
deferred E2E test has a matching log entry AND commit message
body entry. A deferral without both is a Real finding fixed in
Step 4 by adding the missing record.

**Learn phase.** The learn-analyst checks the deferral was
reasonable: the bootstrapping problem named in the log must be
real, the post-merge verification command must be runnable by
the user, and the feature must not be in the
security-sensitive forbidden list.

## Cross-References

- `.claude/rules/always-verify.md` — the broader verification
  discipline that mandates evidence for every change. The
  bootstrapping carve-out here is the only sanctioned
  exception to "verification command must run before
  reporting complete."
- `.claude/rules/plan-commit-atomicity.md` "Plan Signature
  Deviations Must Be Logged" — the sibling rule for the
  `bin/flow log` + commit-message-body record pattern. The
  same dual-record discipline applies to E2E deferrals.
