# Never Add Flags to Failed Merges

When `gh pr merge --squash` fails, diagnose the actual blocker
(missing required checks, branch behind main, conflicts) instead
of adding flags like `--auto` or `--admin`.

- `--auto` enables GitHub auto-merge — a shared-state mutation
- `--admin` bypasses branch protection

Both violate trust. Fix the root cause (merge main, wait for
checks, resolve conflicts) and retry the plain command.
