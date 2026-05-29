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
  FLOW v2.5.0 — release — STARTING
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

## Step 2 — Verify CI, find last release, and gather inputs

Run `git describe` and the two Reads in parallel (one response, one Bash call + two Reads):

```bash
git describe --tags --abbrev=0
```

Also use the Read tool to read `.claude-plugin/plugin.json` and `RELEASE-NOTES.md`.

If `git describe` fails (no tags exist), set `<last_tag>` to `HEAD~20`.

Then verify CI on main is green and finished for the current HEAD. Use a
10-minute Bash tool timeout (`timeout: 600000`) — wait-for-release-ci
blocks until the latest main run reaches a terminal conclusion (polling
for up to ~8 minutes), and the default 2-minute timeout would background
the process, defeating the wait (per `.claude/rules/ci-is-a-gate.md`).

```bash
bin/flow wait-for-release-ci --base main
```

Parse the JSON output and branch on `status`:

- `"ready"` → CI finished for the current HEAD. Check `conclusion`:
  - `"success"` → proceed
  - `"failure"`, `"cancelled"`, or any other terminal conclusion → stop: "CI failed on main. Fix tests before releasing."
- `"still_pending"` → CI did not finish within the cap. Re-run the single `bin/flow wait-for-release-ci --base main` line above (again with the 10-minute Bash tool timeout) until it returns `ready`.
- `"error"` → show the `message` and stop. The latest run may be for a different commit (CI has not run on the latest commit yet), there may be no runs yet, or gh/git failed.

## Step 3 — Show what changed

```bash
git log --oneline <last_tag>..HEAD
```

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

## Step 5 — Bump version and update release notes

Run both in parallel (one response, one Bash call + one Edit):

```bash
make bump NEW=<new_version>
```

Also Edit `RELEASE-NOTES.md` — add the release notes section from Step 4 at the
top (below the `# Release Notes` heading).

The bump updates `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`,
and all skill banners in one step.

Config and setup hashes are not stored in `plugin.json` — they are computed
dynamically by `compute_config_hash()` and `compute_setup_hash()` in
`src/prime_setup.rs` at prime time and compared at start time by
`src/prime_check.rs`. No manual hash updates are needed during releases.

## Step 6 — Rebuild and stage the prebuilt binary

The committed binary at `bin/flow-rs-darwin-arm64` ships to end users
through the marketplace cache — `/plugin install` copies it into place so
a fresh install needs no build step. It must be regenerated from source
at every release so its bytes match the tagged source generation; a
stale binary would run an older FLOW than the release claims.

`bin/setup --stage-binary` builds the release binary and moves it to
the committed path in one step. The move (rather than a copy) leaves
no source artifact at `target/release/flow-rs` after staging — a
leftover would sit at higher dispatcher precedence than the committed
binary at `bin/flow-rs-darwin-arm64` and shadow source changes during
`--plugin-dir` QA on a session that runs without rebuilding (see
`bin/flow` lines 27-33: the dispatcher prefers `target/release` over
the committed binary by existence priority, not mtime). The compiler
and the move both run inside that script, which keeps them off the
FLOW Bash allow-list surface — invoking the Rust toolchain or `mv`
directly is permission-denied. The staging is idempotent: invoking
`--stage-binary` after a prior successful staging (no fresh build
output) leaves the committed binary in place rather than failing.
Use a 10-minute Bash tool timeout (`timeout: 600000`) — a cold
release build can take several minutes and the default 2-minute
timeout would background the process.

```bash
bin/setup --stage-binary
```

After `bin/setup --stage-binary`, `bin/flow-rs-darwin-arm64` is
refreshed in the working tree with the executable bit set, so the
`git add -A` in Step 7 stages the fresh bytes at mode `100755`.

## Step 7 — Stage all changes

```bash
git add -A
```

Staging must happen before writing `.flow-commit-msg` in Step 8 — otherwise
`git add -A` picks up the message file and commits it into the repo.

## Step 8 — Write commit message and finalize

Write `Release v<new_version>` to `.flow-commit-msg` via the Write tool.

Then finalize the commit in one call. `finalize-commit` runs
`ci::run_impl()` before `git commit` (see CLAUDE.md "CI is enforced
inside `finalize-commit` itself"), so use a 10-minute Bash tool
timeout (`timeout: 600000`) — CI runs can take 3–4 minutes and the
default 2-minute timeout would background the process, defeating
the gate (per `.claude/rules/ci-is-a-gate.md`).

```bash
bin/flow finalize-commit .flow-commit-msg main
```

No diff review. No `bin/ci`. No approval prompt — CI was verified in
Step 2, changes were shown in Step 3, and version was confirmed in Step 4.

## Step 9 — Tag, release, and publish

First, run both in parallel (one response, two Bash calls):

```bash
git tag v<new_version>
```

```bash
bin/flow extract-release-notes v<new_version>
```

The extract writes `tmp/release-notes-v<new_version>.md`.

Then run both in parallel (one response, two Bash calls):

```bash
gh release create v<new_version> --title "v<new_version>" --notes-file tmp/release-notes-v<new_version>.md
```

```bash
claude plugin marketplace update flow-marketplace
```

`gh release create` pushes the tag to the remote automatically — no separate
`git push origin` needed.

If the marketplace update fails, print the command for the user to run manually.

## Step 10 — Render Slack notes for the team

After the GitHub release publishes and the marketplace cache refreshes, render a copy-pasteable Slack announcement so the maintainer can post it directly without hand-translating the release notes. This Step turns the GFM-formatted `tmp/release-notes-v<new_version>.md` artifact (already produced by Step 9's `bin/flow extract-release-notes`) into Slack syntax and computes whether `/flow:flow-prime` is needed this release.

Run both in parallel (one response, one Read tool call + one Bash call):

Use the Read tool on `tmp/release-notes-v<new_version>.md`.

```bash
git diff --name-only <last_tag>..HEAD
```

Parse the `git diff` output as a newline-separated file list. Check whether any line equals `src/prime_check.rs`, equals `src/prime_setup.rs`, or starts with `assets/bin-stubs/`. When any match → include the `/flow:flow-prime` line in the footer (the non-empty-diff variant below). When no match → omit it (the empty-diff variant). The path-filter logic lives in this prose rather than as a `-- <paths>` argument on the bash command because `validate-pretool` Layer 6 in `src/hooks/validate_pretool.rs` blocks `git diff` invocations carrying file-path arguments — grep for the `Layer 6` marker in that file to locate the current implementation.

### Translation rules

1. **Lead with user value.** The headline is one sentence naming what shipped this release in user terms — what the user can now do, or what was fixed. Strip release-internal vocabulary.
2. **Subsection mapping.** GFM `### New features` → Slack `*✨ New*`. GFM `### Fixes` → Slack `*🐛 Fixes*`. GFM `### Improvements` → Slack `*💎 Improvements*`. Drop empty subsections entirely (header and all).
3. **Bullet shape.** `- **Title** — body` → `• *Title* — body`. Slack uses single-asterisk bold and the `•` glyph; markdown's double-asterisk and `-` do not render.
4. **Inline code.** Backticks survive the translation as-is — Slack renders `` `name` `` the same way GFM does.
5. **Issue-reference stripping.** Drop every trailing `(#NNNN)` PR reference at the end of a bullet body. The release notes link to GitHub already.
6. **Jargon stripping.** Strip implementation-detail vocabulary readers outside the codebase do not share: layer numbers (e.g. `Layer 10`), internal function names (e.g. `compute_config_hash`, `validate-pretool`), transcript-walker identifiers, hook names, and similar. Rewrite the bullet in business-friendly terms.
7. **Surface count.** Keep the top 2–4 most-impactful bullets per subsection. Relegate remaining items to a single one-line sweep (e.g. "…plus N small fixes and polish.") rather than itemizing every one.
8. **Footer.** Always include the upgrade command. Conditionally include the `/flow:flow-prime` re-prime line per the prime-input check above.

### Footer template

**Empty-diff variant** (no prime-input file changed):

````markdown
```text
*Upgrade:*
`claude plugin marketplace update flow-marketplace`
```
````

**Non-empty-diff variant** (one or more of `src/prime_check.rs`, `src/prime_setup.rs`, or any path under `assets/bin-stubs/` changed):

````markdown
```text
*Upgrade:*
`claude plugin marketplace update flow-marketplace`
*Then re-prime your project:* `/flow:flow-prime`
```
````

### Output

Render the Slack message inline as a single fenced code block, ready for the maintainer to copy. Substitute `<new_version>`, the user-value headline, and the bullet bodies into this template:

````markdown
```text
🚀 *FLOW v<new_version> shipped — <user-value headline>*

*✨ New*
• *<Title>* — <body>
• *<Title>* — <body>

*🐛 Fixes*
• *<Title>* — <body>

*💎 Improvements*
• *<Title>* — <body>

<footer per variant above>
```
````

Drop subsections whose source GFM section was empty. Remove the entire emoji-headed block (header and bullets together) — Slack reads cleaner without a header that has no items underneath.

### Failure handling

If the Read tool on `tmp/release-notes-v<new_version>.md` fails (file missing, permission error, prior `bin/flow extract-release-notes` silently produced nothing), do not block the release. Print a one-line note in your response explaining the skip and proceed to Done. The release itself is already published; only the Slack announcement is missed, and the maintainer can hand-draft from `RELEASE-NOTES.md` if needed.

## Done

Print:

````markdown
```text
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  ✓ FLOW v2.5.0 — release — COMPLETE
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
- Always rebuild and stage the prebuilt binary — `bin/flow-rs-darwin-arm64` must be regenerated from source on every version bump so it never lags the tagged release
- Always tag before pushing — the tag is what humans see on GitHub
- Always create a GitHub Release — it's the public changelog
- Never add Co-Authored-By trailers or attribution lines
- The skill is idempotent: safe to re-run after a `wait-for-release-ci` `still_pending` result, which blocks internally and never leaves partial state
