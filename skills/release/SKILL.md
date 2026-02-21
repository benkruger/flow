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
  FLOW — flow:release — STARTING
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

## Step 3 — Show what changed since last release

Run:

```bash
git log --oneline $(git describe --tags --abbrev=0 2>/dev/null || echo HEAD~20)..HEAD
```

Display the commit list. This is what goes into the release.

## Step 4 — Determine the new version

Read the current version from `.claude-plugin/plugin.json`.

Use AskUserQuestion:

> "Current version is <current_version>. What type of release?"
> - **Patch** — "<major>.<minor>.<patch+1>" (bug fixes, doc corrections)
> - **Minor** — "<major>.<minor+1>.0" (new features, new skills)
> - **Major** — "<major+1>.0.0" (breaking changes)

Display the exact new version number in each option description.

## Step 5 — Bump version in both files

Update the `version` field in both files to the new version:

1. `.claude-plugin/plugin.json` — the `version` field
2. `.claude-plugin/marketplace.json` — the `version` field inside the plugin entry

Both must match exactly. No other fields are changed.

## Step 6 — Update RELEASE-NOTES.md

Read the current `RELEASE-NOTES.md`. Add a new section at the top (below the `# Release Notes` heading) for the new version:

```
## v<new_version> — <short description>

<Summary of what changed — written from the commit list in Step 4.
Group by: new features, fixes, improvements. Be concise.>
```

Use AskUserQuestion to show the draft release notes:

> "Do these release notes look right?"
> - **Yes, looks good**
> - **Needs changes** — describe in Other

## Step 7 — Commit the version bump

Commit exactly these files:

- `.claude-plugin/plugin.json`
- `.claude-plugin/marketplace.json`
- `RELEASE-NOTES.md`

Commit message:

```
Release v<new_version>
```

No body needed — the release notes tell the story.

## Step 8 — Tag and push

```bash
git tag v<new_version>
git push origin main
git push origin v<new_version>
```

## Step 9 — Create GitHub Release

```bash
gh release create v<new_version> \
  --title "v<new_version>" \
  --notes-file RELEASE-NOTES.md
```

Note: `--notes-file` uses the full RELEASE-NOTES.md. GitHub will render it.
If the full file is too long, extract only the section for this version and
pass it via `--notes` with a heredoc instead.

## Done

Print:

```
============================================
  FLOW — flow:release — COMPLETE
  Released v<new_version>
  https://github.com/benkruger/ruby-on-rails-claude-ai-process/releases/tag/v<new_version>
============================================
```

## Rules

- Never release with uncommitted changes
- Never release without showing what changed
- Always bump both plugin.json and marketplace.json — they must match
- Always tag before pushing — the tag is what humans see on GitHub
- Always create a GitHub Release — it's the public changelog
- Never add Co-Authored-By trailers or attribution lines
