---
name: security
description: "Phase 6: Security — scan for security issues in the feature diff. In-flow: diff-only after Review. Standalone: full repo, report-only, no state file required."
model: opus
---

# FLOW Security — Phase 6: Security

## Usage

```text
/flow:security
/flow:security --auto
/flow:security --manual
```

- `/flow:security` — uses configured mode from `.flow.json` (default: auto)
- `/flow:security --auto` — auto-advance to Learning on completion
- `/flow:security --manual` — prompt before advancing to Learning

<HARD-GATE>
Run this phase entry check as your very first action. If any check fails,
stop immediately and show the error to the user.

1. Run both commands in parallel (two Bash calls in one response):
   - `git worktree list --porcelain` — note the path on the first `worktree` line (this is the project root).
   - `git branch --show-current` — this is the current branch.
2. Use the Read tool to read `<project_root>/.flow-states/<branch>.json`.
   - If the file does not exist: STOP. "BLOCKED: No FLOW feature in progress.
     Run /flow:start first."
3. Check `phases.5.status` in the JSON.
   - If not `"complete"`: STOP. "BLOCKED: Phase 5: Review must be
     complete. Run /flow:review first."
</HARD-GATE>

Keep the project root, branch, and state data from the gate in context —
use the project root to build Read tool paths (e.g.
`<project_root>/.flow-states/<branch>.json`). Do not re-read the state
file or re-run git commands to gather the same information. Do not `cd`
to the project root — `bin/flow` commands find paths internally.

## Mode Resolution

1. If `--auto` was passed → commit=auto, continue=auto
2. If `--manual` was passed → commit=manual, continue=manual
3. Otherwise, read `.flow.json` from the project root. Use `skills.security.commit` and `skills.security.continue`.
4. If `.flow.json` has no `skills` key → use built-in defaults: commit=auto, continue=auto

## Announce

At the very start, output the following banner in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.18.0 — Phase 6: Security — STARTING
============================================
```
````

## Update State

Update state for phase entry:

```bash
bin/flow phase-transition --phase 6 --action enter
```

Parse the JSON output to confirm `"status": "ok"`.
If `"status": "error"`, report the error and stop.

## Logging

After every Bash command completes, log it to `.flow-states/<branch>.log`.

Run the command directly — do not append any suffix:

```bash
COMMAND
```

Then Read `.flow-states/<branch>.log` (empty string if it does not
exist yet) and Write it back with this line appended:

```text
YYYY-MM-DDTHH:MM:SSZ [Phase 6] Step X — desc (exit EC)
```

Get `<branch>` from the state file.

## Framework Instructions

Read the `framework` field from the state file and follow only the matching
section below for the security analysis sub-agent prompt. Do not announce
the framework — just follow the matching section silently.

### If Rails

#### Security Analysis Sub-Agent Prompt

Provide these instructions to the Step 1 sub-agent (fill in the details):

> You are analyzing a feature diff for security issues in a Ruby on Rails
> application.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for git commands
> (git diff, git log, git blame, git show). Never pipe git output through
> sed, grep, awk, or any other command — read the full output and extract
> what you need in your response. Never use `cd <path> && git` — run git
> commands from the current directory. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Plan file:
> <paste the plan file contents — includes approach and risks>
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

### If Python

#### Security Analysis Sub-Agent Prompt

Provide these instructions to the Step 1 sub-agent (fill in the details):

> You are analyzing a feature diff for security issues in a Python
> application.
> Feature: <feature name from state>
>
> **Tool rules:** Use Glob and Read tools for all file and directory
> operations. Use Grep for searching code. Only use Bash for git commands
> (git diff, git log, git blame, git show). Never pipe git output through
> sed, grep, awk, or any other command — read the full output and extract
> what you need in your response. Never use `cd <path> && git` — run git
> commands from the current directory. Never use Bash for any other
> purpose — no find, ls, cat, wc, test -f, stat, or running project tooling.
>
> Plan file:
> <paste the plan file contents — includes approach and risks>
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
> **Check 1: Command injection** (`command_injection`)
> Look for `subprocess.run`, `subprocess.call`, `os.system`,
> `os.popen`, or `Popen` where command arguments come from user input
> or external data without proper escaping.
> Vulnerable: `subprocess.run(f"echo {user_input}", shell=True)`
> Safe: `subprocess.run(["echo", user_input])`
>
> **Check 2: Path traversal** (`path_traversal`)
> Look for file operations where the path includes user input without
> validation. Check for `../` traversal or absolute path injection.
> Vulnerable: `open(f"uploads/{filename}")`
> Safe: `path.resolve().relative_to(base_dir)`
>
> **Check 3: Input validation** (`input_validation`)
> Look for external inputs (CLI args, environment variables, file
> contents, API responses) used without validation or sanitization.
> Vulnerable: `int(sys.argv[1])` without try/except
> Vulnerable: `os.environ["SECRET"]` without fallback
>
> **Check 4: Data exposure** (`data_exposure`)
> Look for sensitive data (passwords, tokens, secrets, API keys) logged,
> printed, or written to files. Check for credentials hardcoded in source.
> Vulnerable: `print(f"Token: {token}")`
> Vulnerable: `API_KEY = "sk-abc123"`
>
> **Check 5: Unsafe deserialization** (`unsafe_deserialization`)
> Look for `pickle.load`, `yaml.load` (without SafeLoader), `eval`,
> `exec`, or `__import__` on untrusted data.
> Vulnerable: `pickle.load(user_file)`
> Vulnerable: `yaml.load(data)` (without `Loader=SafeLoader`)
> Safe: `json.loads(data)`
>
> **Check 6: Dependency security** (`dependency_security`)
> Look for new dependencies added without version pinning, or known
> vulnerable versions. Check `requirements.txt`, `pyproject.toml`.
> Vulnerable: `requests` (no version pin)
> Safe: `requests>=2.31.0`
>
> **Check 7: Error information leakage** (`error_leakage`)
> Look for exception handling that exposes internal details (stack
> traces, file paths, database queries) to external consumers.
> Vulnerable: `return str(e)` in an API response
> Safe: `return "Internal error"` with logging of the full exception
>
> **Check 8: Lint suppression** (`lint_suppression`)
> Look for any `# noqa`, `# type: ignore`, or `# pragma: no cover`
> comment in the diff. Each is a finding — remove it and fix the
> underlying issue.
>
> **Check 9: Temporary file safety** (`temp_file_safety`)
> Look for `open("/tmp/...")` or predictable temporary file names.
> Vulnerable: `open("/tmp/myapp_data.txt", "w")`
> Safe: `tempfile.NamedTemporaryFile()`
>
> **Check 10: Permission and access** (`permission_access`)
> Look for file permission changes, `chmod`, or files created with
> overly permissive modes. Check for `os.chmod(path, 0o777)` or
> similar.
>
> Return your findings as two lists:
>
> **Findings** — for each issue found:
>
> - Check name and key (e.g., "Command injection" / `command_injection`)
> - Description of the specific issue
> - File path and line number
>
> **Clean checks** — list the check keys that found no issues.
>
> If no issues are found across all checks, say so explicitly and list
> all 10 checks as clean.

---

## Step 1 — Launch security analysis sub-agent

Read `plan_file` from the state file to get the plan file path. Use the
Read tool to read the plan file — it contains the approach and risks.

Then launch a mandatory sub-agent to analyze the feature diff for security
issues. Use the Task tool:

- `subagent_type`: `"general-purpose"`
- `description`: `"Security analysis"`

Provide the sub-agent with the **Security Analysis Sub-Agent Prompt** from the
framework section above (fill in the feature name and plan file contents).

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
      "check": "<check_name>",
      "description": "<what was found and where>",
      "file": "<path/to/affected_file>",
      "line": 15,
      "status": "pending"
    }
  ],
  "clean_checks": ["<check_1>", "<check_2>", "<check_3>"],
  "scanned_at": null
}
```

Check names and categories are defined by the framework section above.

Number each finding with a sequential `id`. Set `status` to `"pending"` for
every confirmed finding. Set `scanned_at` to `null` in the object you write.

If there are no confirmed findings, set `findings` to an empty array, list
all 10 checks in `clean_checks`, and skip to Step 4.

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

Then set the scan timestamp:

```bash
bin/flow set-timestamp --set security.scanned_at=NOW
```

---

## Step 3 — Fix findings

Fix each confirmed finding one at a time, in order:

1. Fix the issue in code
2. Run `bin/flow ci --if-dirty`
3. If commit=auto, invoke `/flow:commit --auto` for the fix. Otherwise invoke `/flow:commit`.
4. Update the finding's `status` to `"fixed"` in the state file
5. Move to the next finding

**How to update:** Read `.flow-states/<branch>.json`, parse the JSON,
modify the fields in memory, then use the Write tool to write the
entire file back. Never use the Edit tool for state file changes —
field names repeat across phases and cause non-unique match errors.

<HARD-GATE>
`bin/flow ci` must be green after every fix. Do not move to the next finding
until the current fix passes `bin/flow ci` and is committed.
</HARD-GATE>

Repeat until all findings have `status: "fixed"`.

---

## Step 4 — Present security summary

Show a summary of what was found and fixed inside a fenced code block:

````markdown
```text
============================================
  FLOW — Phase 6: Security — SUMMARY
============================================

  Checks run       : 10
  Findings         : N
  Fixed            : N
  Clean checks     : N

  Findings
  --------
  - [FIXED] <check_name>: <description of finding>
  - [FIXED] <check_name>: <description of finding>

  Clean Checks
  ------------
  <check_1>, <check_2>, <check_3>, ...

  bin/flow ci      : ✓ green

============================================
```
````

---

## Done — Update state and complete phase

Complete the phase:

```bash
bin/flow phase-transition --phase 6 --action complete
```

Parse the JSON output. If `"status": "error"`, report the error and stop.
Use the `formatted_time` field in the COMPLETE banner below. Do not print
the timing calculation.

Output in your response (not via Bash) inside a fenced code block:

````markdown
```text
============================================
  FLOW v0.18.0 — Phase 6: Security — COMPLETE (<formatted_time>)
============================================
```
````

Invoke `flow:status`.

**If continue=auto**, skip the transition question and invoke `flow:learning` directly.

**If continue=manual**, use AskUserQuestion:

> "Phase 6: Security is complete. Ready to begin Phase 7: Learning?"
>
> - **Yes, start Phase 7 now** — invoke `flow:learning`
> - **Not yet** — print paused banner
> - **I have a correction or learning to capture**

**If "I have a correction or learning to capture":**
1. Ask the user what they want to capture
2. Invoke `/flow:note` with their message
3. Re-ask with only "Yes, start Phase 7 now" and "Not yet"

**If Yes** — invoke `flow:learning` using the Skill tool.

**If Not yet**, output in your response (not via Bash) inside a fenced code block:

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

- Always run `bin/flow ci` after any fix made during Security
- Never transition to Learning unless `bin/flow ci` is green
- Read the full diff before starting — no partial reviews
- Never use Bash to print banners — output them as text in your response
- Never use Bash for file reads — use Glob, Read, and Grep tools instead of ls, cat, head, tail, find, or grep
- Never use `cd <path> && git` — use `git -C <path>` for git commands in other directories
- Never cd before running `bin/flow` — it detects the project root internally
