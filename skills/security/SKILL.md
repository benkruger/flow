---
name: security
description: "Phase 7: Security — scan for security issues in the feature diff. In-flow: diff-only after Review. Standalone: full repo, report-only, no state file required."
model: opus
---

# FLOW Security — Phase 7: Security

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.6.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 6: Review must be
     complete. Run /flow:review first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
all subsequent steps use them directly. Do not re-read the state file or
re-run git commands to gather the same information.

## Announce

At the very start, print inside a fenced code block (triple backticks) so it renders as plain monospace text and not as a markdown heading:

````markdown
```text
============================================
  FLOW v0.10.0 — Phase 7: Security — STARTING
============================================
```
````

## Update State

Using the state data from the gate, cd into the worktree and update Phase 7:
- `status` → `in_progress`
- `started_at` → current UTC timestamp (only if null — never overwrite)
- `session_started_at` → current UTC timestamp
- `visit_count` → increment by 1
- `current_phase` → `7`

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 7] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

---

## Step 1 — Launch security analysis sub-agent

Read the following from the state file (small, structured — keep in main context):
- `state["design"]` — what was approved to be built
- `state["research"]["risks"]` — risks identified during Research

Then launch a mandatory sub-agent to analyze the feature diff for security
issues. Use the Task tool:

- `subagent_type`: `"Explore"`
- `description`: `"Security analysis"`

Provide these instructions to the sub-agent (fill in the details):

> You are analyzing a feature diff for security issues in a Ruby on Rails
> application.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory checks.
> Use Grep for searching code. Only use Bash for git commands (git diff,
> git log, git blame). Never use Bash for file existence checks, directory
> listings, or reading file contents (`test -f`, `ls`, `cat`, etc.).
>
> Approved design:
> <paste state["design"] — chosen_approach, schema_changes, model_changes,
> controller_changes, worker_changes, route_changes>
>
> Research risks:
> <paste state["research"]["risks"]>
>
> First, get the full diff:
>
> ```bash
> git diff origin/main...HEAD
> ```
>
> Read every changed file completely. Then run each of these 10 security
> checks against the diff. For each check, report either a finding (with
> file path and line number) or mark it clean.
>
> **Check 1: Authorization gaps** (`authorization_gaps`)
> Look for controller actions added or modified in the diff that have no
> `before_action` authentication/authorization filter. Check whether
> existing auth filters are skipped for new actions.
> Vulnerable pattern: a new action with no auth filter covering it
> Vulnerable pattern: `skip_before_action :require_login, only: [:new_action]`
>
> **Check 2: Unscoped record access** (`unscoped_access`)
> Look for record lookups that use `params[:id]` (or similar) without
> scoping to the current user, account, or tenant.
> Vulnerable: `Record::Base.find(params[:id])`
> Safe: `current_account.records.find(params[:id])`
>
> **Check 3: Mass assignment** (`mass_assignment`)
> Look for `params.permit!` (permits everything), overly broad `permit`
> calls that include admin-only or internal fields, or params hashes
> passed directly to `create!`/`update!` without `permit`.
> Vulnerable: `Record::Create.create!(params[:record])`
> Vulnerable: `params.require(:record).permit!`
> Vulnerable: `permit(:role, :admin, :internal_flag)` when user-facing
>
> **Check 4: SQL injection** (`sql_injection`)
> Look for string interpolation or concatenation inside `where`, `order`,
> `select`, `group`, `having`, `pluck`, `execute`, `find_by_sql`, or
> `ActiveRecord::Base.connection` calls.
> Vulnerable: `where("name = '#{params[:name]}'")`
> Vulnerable: `order("#{params[:sort]} #{params[:dir]}")`
> Safe: `where(name: params[:name])`
> Safe: `where("name = ?", params[:name])`
>
> **Check 5: Data exposure** (`data_exposure`)
> Look for sensitive fields (password, token, secret, ssn, credit card,
> api\_key) included in `as_json`, `to_json`, serializer attributes,
> or API responses. Check for PII logged via `Rails.logger` or `puts`.
> Check for credentials or secrets hardcoded in source.
> Vulnerable: `render json: user.as_json`
> Vulnerable: `Rails.logger.info("User #{user.email} token: #{user.token}")`
>
> **Check 6: CSRF bypass** (`csrf_bypass`)
> Look for `skip_before_action :verify_authenticity_token`. This is only
> acceptable on API-only controllers that use token auth instead of
> cookies. If the controller serves browser requests with cookie auth,
> this is a finding.
> Vulnerable: `skip_before_action :verify_authenticity_token` on a
> controller that uses session/cookie auth
>
> **Check 7: Open redirects** (`open_redirects`)
> Look for `redirect_to` where the URL comes from user input (params,
> form fields, headers). Attackers use this for phishing.
> Vulnerable: `redirect_to params[:return_url]`
> Vulnerable: `redirect_to request.referer`
> Safe: `redirect_to root_path`
> Safe: `redirect_to params[:return_url], allow_other_host: false`
>
> **Check 8: RuboCop disables** (`rubocop_disables`)
> Look for any `# rubocop:disable` comment anywhere in the diff. This is
> an automatic finding — no judgment needed. Every disable must be
> removed and the underlying code fixed.
>
> **Check 9: Auth test coverage** (`auth_test_coverage`)
> If the diff adds a `before_action` auth check or an authorization
> gate, look for a corresponding test that verifies the unauthorized
> or forbidden case. A new auth check without a test for the reject
> path is a finding.
>
> **Check 10: Route exposure** (`route_exposure`)
> Look for new routes (in `config/routes.rb` or `config/routes/*.rb`)
> that point to controller actions. Read the target controller and
> verify it has an auth filter covering the routed action. A route to
> an unprotected action is a finding.
>
> Return your findings as two lists:
>
> **Findings** — for each issue found:
>
> - Check name and key (e.g., "Authorization gaps" / `authorization_gaps`)
> - Description of the specific issue
> - File path and line number
>
> **Clean checks** — list the check keys that found no issues.
>
> If no issues are found across all checks, say so explicitly and list
> all 10 checks as clean.

Wait for the sub-agent to return before proceeding.

---

## Step 2 — Confirm findings and record in state

Read the sub-agent's findings. For each reported issue:

1. Read the cited file and line to confirm the issue exists (sub-agents may
   have false positives)
2. Drop any finding that is a false positive — explain why it was dropped

Write all confirmed findings and clean checks to the state file:

```json
"security": {
  "findings": [
    {
      "id": 1,
      "check": "authorization_gaps",
      "description": "PaymentController#show has no before_action auth check",
      "file": "app/controllers/payment_controller.rb",
      "line": 15,
      "status": "pending"
    }
  ],
  "clean_checks": ["sql_injection", "csrf_bypass", "open_redirects"],
  "scanned_at": "2026-02-20T15:00:00Z"
}
```

Number each finding with a sequential `id`. Set `status` to `"pending"` for
every confirmed finding. `scanned_at` is the current UTC timestamp.

If there are no confirmed findings, set `findings` to an empty array, list
all 10 checks in `clean_checks`, and skip to Step 4.

---

## Step 3 — Fix findings

Fix each confirmed finding one at a time, in order:

1. Fix the issue in code
2. Run `bin/ci`
3. Invoke `/flow:commit` for the fix
4. Update the finding's `status` to `"fixed"` in the state file
5. Move to the next finding

<HARD-GATE>
bin/ci must be green after every fix. Do not move to the next finding
until the current fix passes bin/ci and is committed.
</HARD-GATE>

Repeat until all findings have `status: "fixed"`.

---

## Step 4 — Present security summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 7: Security — SUMMARY
============================================

  Checks run       : 10
  Findings         : N
  Fixed            : N
  Clean checks     : N

  Findings
  --------
  - [FIXED] authorization_gaps: PaymentController#show has no auth check
  - [FIXED] rubocop_disables: # rubocop:disable in payment_controller.rb

  Clean Checks
  ------------
  sql_injection, csrf_bypass, open_redirects, ...

  bin/ci           : ✓ green

============================================
```
````

---

## Done — Update state and complete phase

Update Phase 7 in state:
1. `cumulative_seconds` += `current_time - session_started_at`. Do not print the calculation.
2. `status` → `complete`
3. `completed_at` → current UTC timestamp
4. `session_started_at` → `null`
5. `current_phase` → `8`

Format `cumulative_seconds` as `<formatted_time>`: `Xh Ym` if ≥ 3600, `Xm` if ≥ 60, `<1m` if < 60.

Print inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.10.0 — Phase 7: Security — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`, then use AskUserQuestion:

> "Phase 7: Security is complete. Ready to begin Phase 8: Reflect?"
>
> - **Yes, start Phase 8 now** — invoke `flow:reflect`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 8 now" and "Not yet"

**If Yes** — invoke `flow:reflect` using the Skill tool.

**If Not yet**, print inside a fenced code block:

````markdown
```text
============================================
  FLOW — Paused
  Run /flow:continue when ready to continue.
============================================
```
````

---

## Hard Rules

- Always run `bin/ci` after any fix made during Security
- Never transition to Reflect unless bin/ci is green
- Read the full diff before starting — no partial reviews
