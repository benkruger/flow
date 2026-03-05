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
  FLOW v0.15.0 — release — STARTING
============================================
```

## Flags

**Default (no flags):** Auto-detect version, display version and release notes, then proceed directly to Step 6 without approval.

**`--manual`:** Pause at Step 5 for approval. Allows overriding the version or editing release notes. Also serves as a dry-run — deny at the prompt to stop.

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
- `null` (in_progress/queued) → poll: sleep 15 seconds, re-check, up to 3 retries
  (45 seconds total). Print "CI still running... checking again in 15s (attempt N/3)"
  each time. If still not done after 3 attempts, stop: "CI hasn't finished after
  45 seconds. Check GitHub Actions manually."

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

## Step 5 — Determine version and draft release notes

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

Then draft the release notes section:

```
## v<new_version> — <short description>

<Summary of what changed — written from the commit list in Step 4.
Group by: new features, fixes, improvements. Be concise.>
```

Present the recommendation and the draft release notes in your response.

**If `--manual` was passed:** use one AskUserQuestion:

> "I recommend **<type>** (v<new_version>) — <one sentence reason>.
>  Release notes are above. Approve this release?"
> - **Approve** (Recommended)
> - **Different version** — specify in Other
> - **Notes need changes** — describe in Other

**Default (no flags):** proceed directly to Step 6.

## Step 6 — Bump version in all files

Run:

```bash
make bump NEW=<new_version>
```

This updates `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`,
and all skill banners in one step.

## Step 7 — Update RELEASE-NOTES.md

Read the current `RELEASE-NOTES.md`. Add the release notes section
approved in Step 5 at the top (below the `# Release Notes` heading).

## Step 8 — Commit and push

```bash
git add -A
```

Write `Release v<new_version>` to `.flow-commit-msg` via the Write tool, then:

```bash
git commit -F .flow-commit-msg
```

```bash
rm .flow-commit-msg
```

```bash
git pull origin main
```

```bash
git push origin main
```

No diff review. No `bin/ci`. No approval prompt — CI was verified in
Step 3, changes were shown in Step 4, and version was confirmed in Step 5.

## Step 9 — Tag and push

```bash
git tag v<new_version>
git push origin main
git push origin v<new_version>
```

## Step 10 — Create GitHub Release

First extract just this version's section from RELEASE-NOTES.md:

```bash
bin/flow extract-release-notes v<new_version>
```

This writes `/tmp/release-notes-v<new_version>.md`. Then create the release:

```bash
gh release create v<new_version> --title "v<new_version>" --notes-file /tmp/release-notes-v<new_version>.md
```

## Step 11 — Update local marketplace

```bash
claude plugin marketplace update flow-marketplace
```

If this fails, print the command for the user to run manually.

## Done

Print:

```
============================================
  FLOW v0.15.0 — release — COMPLETE
  Released v<new_version>
  https://github.com/benkruger/flow/releases/tag/v<new_version>

  Local plugin upgraded:
  claude plugin marketplace update flow-marketplace
============================================
```

## Rules

- Never release with uncommitted changes
- Never release without showing what changed
- Always bump both plugin.json and marketplace.json — they must match
- Always tag before pushing — the tag is what humans see on GitHub
- Always create a GitHub Release — it's the public changelog
- Never add Co-Authored-By trailers or attribution lines
- `--manual` is user-invoked only. Claude must never pass this flag programmatically.
