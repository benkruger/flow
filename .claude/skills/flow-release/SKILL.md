---
name: flow-release
description: "Release a new version of the FLOW plugin. Bumps version in plugin.json and marketplace.json, commits, tags, pushes, and creates a GitHub Release."
---

# FLOW Release

Release a new version of the FLOW plugin. Maintainer-only — requires push access to the repo.

## Announce

Print:

````markdown
```text
──────────────────────────────────────────────────
  FLOW v0.33.0 — release — STARTING
──────────────────────────────────────────────────
```
````

## Flags

**Default (no flags):** Auto-detect version, display version and release notes, then proceed directly from Step 4 to Step 5.

**`--auto`:** Same as default (explicit flag for clarity).

**`--manual`:** Pause at Step 4 for approval before bumping. Serves as a dry-run — deny at the prompt to stop.

## Step 1 — Pre-flight checks

Run both in parallel (one response, two Bash calls):

```bash
git status
```

```bash
git pull origin main
```

If `git status` shows uncommitted changes, stop:

> "There are uncommitted changes. Commit or stash them before releasing."

If `git pull` produced changes, warn the user that new commits were pulled.

## Step 2 — Verify CI and find last release

Run all three in parallel (one response, three Bash calls):

```bash
gh run list --branch main --limit 1 --json conclusion,headSha,status
```

```bash
git rev-parse HEAD
```

```bash
git describe --tags --abbrev=0
```

First verify the run's `headSha` matches `git rev-parse HEAD`.
If not, CI hasn't run on the latest commit — tell the user and stop.

Then check `conclusion`:

- `"success"` → proceed
- `"failure"` or `"cancelled"` → stop: "CI failed on main. Fix tests before releasing."
- `null` (in_progress/queued) → invoke the `loop` skill via the Skill tool with args `15s /flow-release` and return. The loop will re-invoke the release skill automatically until CI completes.

If `git describe` fails (no tags exist), set `<last_tag>` to `HEAD~20`.

## Step 3 — Show what changed and read version

Run both in parallel (one response, one Bash call + one Read):

```bash
git log --oneline <last_tag>..HEAD
```

Also use the Read tool to read `.claude-plugin/plugin.json`.

Display the commit list. This is what goes into the release.

**Do not stop here.** The tag name matching the current version does NOT
mean there is nothing to release — the tag may point to an older commit.

**Only if the commit list is empty** (no output from `git log`), stop:

> "Nothing to release — HEAD is already tagged as `<last_tag>`."

## Step 4 — Determine version and draft release notes

Analyze the commit list from Step 3 and recommend a release type using
these rules (apply the highest that matches):

- **Major** — any commit removes or renames a skill, changes a skill's
  invocation command, or breaks backwards compatibility with existing
  state files
- **Minor** — any commit adds a new skill, adds a new phase, or adds
  significant new behaviour to an existing skill
- **Patch** — all commits are bug fixes, doc corrections, wording
  improvements, or permission/config tweaks

Then draft the release notes section:

````markdown
```text
## v<new_version> — <short description>

<Summary of what changed — written from the commit list in Step 3.
Group by: new features, fixes, improvements. Be concise.>
```
````

Present the recommendation and the draft release notes in your response.

**If `--manual` was explicitly passed**, use one AskUserQuestion:

> "I recommend **<type>** (v<new_version>) — <one sentence reason>.
>  Release notes are above. Approve this release?"
> - **Approve** (Recommended)
> - **Different version** — specify in Other
> - **Notes need changes** — describe in Other

**Unless `--manual` was explicitly passed**, proceed directly to Step 5.

## Step 5 — Bump version and prepare release notes

Run both in parallel (one response, one Bash call + one Read):

```bash
make bump NEW=<new_version>
```

Also use the Read tool to read `RELEASE-NOTES.md`.

The bump updates `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`,
and all skill banners in one step.

Config and setup hashes are not stored in `plugin.json` — they are computed
dynamically by `compute_config_hash()` and `compute_setup_hash()` in
`lib/prime-setup.py` at prime time and compared at start time by
`lib/prime-check.py`. No manual hash updates are needed during releases.

## Step 6 — Update release notes and stage

Edit `RELEASE-NOTES.md` — add the release notes section from Step 4 at the
top (below the `# Release Notes` heading).

Then stage all changes:

```bash
git add -A
```

Staging must happen before writing `.flow-commit-msg` in Step 7 — otherwise
`git add -A` picks up the message file and commits it into the repo.

## Step 7 — Write commit message and finalize

Write `Release v<new_version>` to `.flow-commit-msg` via the Write tool.

Then finalize the commit in one call:

```bash
bin/flow finalize-commit .flow-commit-msg main
```

No diff review. No `bin/ci`. No approval prompt — CI was verified in
Step 2, changes were shown in Step 3, and version was confirmed in Step 4.

## Step 8 — Tag and extract release notes

Run both in parallel (one response, two Bash calls):

```bash
git tag v<new_version>
```

```bash
bin/flow extract-release-notes v<new_version>
```

The extract writes `tmp/release-notes-v<new_version>.md`.

## Step 9 — Push tag

```bash
git push origin v<new_version>
```

## Step 10 — Publish release and update marketplace

Run both in parallel (one response, two Bash calls):

```bash
gh release create v<new_version> --title "v<new_version>" --notes-file tmp/release-notes-v<new_version>.md
```

```bash
claude plugin marketplace update flow-marketplace
```

If the marketplace update fails, print the command for the user to run manually.

## Done

Print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v0.33.0 — release — COMPLETE
  Released v<new_version>
  https://github.com/benkruger/flow/releases/tag/v<new_version>

  Local plugin upgraded:
  claude plugin marketplace update flow-marketplace
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```
````

## Rules

- Never release with uncommitted changes
- Never release without showing what changed
- Always bump both plugin.json and marketplace.json — they must match
- Always tag before pushing — the tag is what humans see on GitHub
- Always create a GitHub Release — it's the public changelog
- Never add Co-Authored-By trailers or attribution lines
- The skill is idempotent: safe to re-invoke via `/loop` after a "pending CI" stop
