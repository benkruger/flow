# Code Review Scope

## In-Scope vs Tech Debt

When an agent (adversarial, reviewer, pre-mortem) finds a bug in a
function that the current PR modifies, the bug is in-scope — fix it
directly in the PR. Do not file it as tech debt.

Tech debt is only for bugs in code the PR does not touch. The
boundary is the function, not the line: if the PR edits any line
in the function, every bug in that function is in-scope.
