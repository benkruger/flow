---
name: release
description: "Release a new version of the FLOW plugin. Bumps version in plugin.json and marketplace.json, commits, tags, pushes, and creates a GitHub Release."
---

# FLOW Release

Release a new version of the FLOW plugin. Maintainer-only — requires push access to the repo.

## Announce

Print:

```
============================================
  FLOW — release — STARTING
============================================
```

## Step 1 — Check for uncommitted changes

Run `git status`. If there are uncommitted changes, stop:

> "There are uncommitted changes. Commit or stash them before releasing."

Do not proceed until the working tree is clean.

## Step 2 — Check main is up to date

```bash
git pull origin main
```

If this produces changes, warn the user that new commits were pulled.

## Step 3 — Verify CI is green

Run:

```bash
gh run list --branch main --limit 1 --json conclusion,headSha,status
```

First verify the run's `headSha` matches `git rev-parse HEAD`.
If not, CI hasn't run on the latest commit — tell the user and stop.

Then check `conclusion`:

- `"success"` → proceed
- `"failure"` or `"cancelled"` → stop: "CI failed on main. Fix tests before releasing."
- `null` (in_progress/queued) → poll: sleep 30 seconds, re-check, up to 3 retries
  (90 seconds total). Print "CI still running... checking again in 30s (attempt N/3)"
  each time. If still not done after 3 attempts, stop: "CI hasn't finished after
  90 seconds. Check GitHub Actions manually."

## Step 4 — Show what changed since last release

Run these two commands separately:

```bash
git describe --tags --abbrev=0
```

If that succeeds, use the tag it returns as `<last_tag>` and run:

```bash
git log --oneline <last_tag>..HEAD
```

If `git describe` fails (no tags exist), run:

```bash
git log --oneline HEAD~20..HEAD
```

Display the commit list. This is what goes into the release.

## Step 5 — Determine the new version

Read the current version from `.claude-plugin/plugin.json`.

Analyze the commit list from Step 4 and recommend a release type using
these rules (apply the highest that matches):

- **Major** — any commit removes or renames a skill, changes a skill's
  invocation command, or breaks backwards compatibility with existing
  state files
- **Minor** — any commit adds a new skill, adds a new phase, or adds
  significant new behaviour to an existing skill
- **Patch** — all commits are bug fixes, doc corrections, wording
  improvements, or permission/config tweaks

State your recommendation and the one-line reason before asking.

Use AskUserQuestion:

> "I recommend **<type>** (<new_version>) — <one sentence reason>.
>  Confirm the release type:"
> - **<Recommended type>** — "<new_version>" (Recommended)
> - **Patch** — "<major>.<minor>.<patch+1>"
> - **Minor** — "<major>.<minor+1>.0"
> - **Major** — "<major+1>.0.0"

Put the recommended type first in the list. Show all three options so
the user can override.

## Step 6 — Bump version in all files

Update the version string in all four files to the new version:

1. `.claude-plugin/plugin.json` — the `version` field
2. `.claude-plugin/marketplace.json` — the `version` field inside the plugin entry
3. `skills/start/SKILL.md` — the version in the announce banner (`FLOW v<version>`)
4. `skills/status/SKILL.md` — the version in the status panel header (`FLOW v<version>`)

All four must match exactly. Use `replace_all` to find and replace the old
version string `FLOW v<old_version>` with `FLOW v<new_version>` in the skill
files. No other fields or content are changed.

## Step 7 — Update RELEASE-NOTES.md

Read the current `RELEASE-NOTES.md`. Add a new section at the top (below the `# Release Notes` heading) for the new version:

```
## v<new_version> — <short description>

<Summary of what changed — written from the commit list in Step 5.
Group by: new features, fixes, improvements. Be concise.>
```

Use AskUserQuestion to show the draft release notes:

> "Do these release notes look right?"
> - **Yes, looks good**
> - **Needs changes** — describe in Other

## Step 8 — Commit the version bump

Commit exactly these files:

- `.claude-plugin/plugin.json`
- `.claude-plugin/marketplace.json`
- `skills/start/SKILL.md`
- `skills/status/SKILL.md`
- `RELEASE-NOTES.md`

Commit message:

```
Release v<new_version>
```

No body needed — the release notes tell the story.

## Step 9 — Tag and push

```bash
git tag v<new_version>
git push origin main
git push origin v<new_version>
```

## Step 10 — Create GitHub Release

First extract just this version's section from RELEASE-NOTES.md:

```bash
python3 hooks/extract-release-notes.py v<new_version>
```

This writes `/tmp/release-notes-v<new_version>.md`. Then create the release:

```bash
gh release create v<new_version> --title "v<new_version>" --notes-file /tmp/release-notes-v<new_version>.md
```

## Done

Print:

```
============================================
  FLOW — release — COMPLETE
  Released v<new_version>
  https://github.com/benkruger/flow/releases/tag/v<new_version>

  Upgrade: claude plugin marketplace update flow-marketplace
============================================
```

## Rules

- Never release with uncommitted changes
- Never release without showing what changed
- Always bump both plugin.json and marketplace.json — they must match
- Always tag before pushing — the tag is what humans see on GitHub
- Always create a GitHub Release — it's the public changelog
- Never add Co-Authored-By trailers or attribution lines
