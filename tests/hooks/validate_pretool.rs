//! Integration tests for `src/hooks/validate_pretool.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

use flow_rs::flow_paths::finalize_commit_destination;
use flow_rs::hooks::validate_pretool::{should_block_background, validate, validate_agent};
use serde_json::{json, Value};

fn sample_settings() -> Value {
    json!({
        "permissions": {
            "allow": [
                "Bash(git status)",
                "Bash(git diff *)",
                "Bash(*bin/*)",
            ],
            "deny": []
        }
    })
}

fn deny_settings() -> Value {
    json!({
        "permissions": {
            "allow": ["Bash(git *)"],
            "deny": [
                "Bash(git rebase *)",
                "Bash(git push --force *)",
                "Bash(git push -f *)",
                "Bash(git reset --hard *)",
                "Bash(git stash *)",
                "Bash(git checkout *)",
                "Bash(git clean *)",
            ]
        }
    })
}

// --- Basic allow tests ---

#[test]
fn test_allows_bin_flow_ci() {
    let (allowed, msg) = validate("bin/flow ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_bin_ci() {
    let (allowed, msg) = validate("bin/ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_add() {
    let (allowed, msg) = validate("git add -A", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_diff() {
    let (allowed, msg) = validate("git diff HEAD", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_empty_command() {
    let (allowed, msg) = validate("", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Compound command blocking ---

#[test]
fn test_blocks_compound_and() {
    let (allowed, msg) = validate("cd .worktrees/test && git status", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
    assert!(msg.contains("separate Bash calls"));
}

#[test]
fn test_blocks_compound_semicolon() {
    let (allowed, msg) = validate("bin/ci; echo done", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_blocks_pipe() {
    let (allowed, msg) = validate("git show HEAD:file.py | sed 's/foo/bar/'", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
    assert!(msg.contains("separate Bash calls"));
}

#[test]
fn test_blocks_or_operator() {
    let (allowed, msg) = validate("bin/ci || echo failed", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

// --- Exec prefix ---

#[test]
fn test_blocks_exec_prefix() {
    let (allowed, msg) = validate("exec /Users/ben/code/flow/bin/flow ci", None, true);
    assert!(!allowed);
    assert!(msg.contains("exec"));
    assert!(msg.contains("permission prompt"));
}

#[test]
fn test_blocks_exec_bare_command() {
    let (allowed, msg) = validate("exec bin/flow ci", None, true);
    assert!(!allowed);
    assert!(msg.contains("exec"));
}

#[test]
fn test_allows_command_without_exec() {
    let (allowed, msg) = validate("/Users/ben/code/flow/bin/flow ci", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Blanket restore ---

#[test]
fn test_blocks_git_restore_dot() {
    let (allowed, msg) = validate("git restore .", None, true);
    assert!(!allowed);
    assert!(msg.contains("git restore ."));
    assert!(msg.contains("individually"));
}

#[test]
fn test_allows_git_restore_specific_file() {
    let (allowed, msg) = validate("git restore lib/foo.py", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Git diff with file args ---

#[test]
fn test_blocks_git_diff_with_file_args() {
    let (allowed, msg) = validate("git diff origin/main..HEAD -- file.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("Read"));
}

#[test]
fn test_blocks_git_diff_head_with_file_args() {
    let (allowed, msg) = validate("git diff HEAD -- src/lib/foo.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_git_diff_cached_with_file_args() {
    let (allowed, msg) = validate("git diff --cached -- file.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_allows_git_diff_without_file_args() {
    let (allowed, msg) = validate("git diff origin/main..HEAD", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_git_diff_stat() {
    let (allowed, msg) = validate("git diff --stat", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- Whitelist ---

#[test]
fn test_whitelist_allows_matching_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_allows_glob_match() {
    let s = sample_settings();
    let (allowed, msg) = validate("git diff HEAD", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_allows_bin_glob() {
    let s = sample_settings();
    let (allowed, _) = validate("bin/ci", Some(&s), true);
    assert!(allowed);
}

#[test]
fn test_whitelist_allows_leading_glob() {
    let s = sample_settings();
    let (allowed, _) = validate("/usr/local/bin/flow ci", Some(&s), true);
    assert!(allowed);
}

#[test]
fn test_whitelist_allows_chmod_absolute_path() {
    let s = json!({"permissions": {"allow": ["Bash(chmod +x *)"], "deny": []}});
    let (allowed, msg) = validate(
        "chmod +x /Users/ben/code/hh/.worktrees/feature/bin/qa",
        Some(&s),
        true,
    );
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_blocks_unmatched_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("curl http://example.com", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
    assert!(msg.contains("curl http://example.com"));
}

#[test]
fn test_whitelist_blocks_rm_rf() {
    let s = sample_settings();
    let (allowed, msg) = validate("rm -rf /", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_whitelist_skipped_when_no_settings() {
    let (allowed, msg) = validate("curl http://example.com", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_whitelist_skipped_when_empty_allow() {
    let s = json!({"permissions": {"allow": []}});
    let (allowed, _) = validate("curl http://example.com", Some(&s), true);
    assert!(allowed);
}

// --- flow_active parameter ---

#[test]
fn test_flow_active_false_allows_unlisted_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_flow_active_true_blocks_unlisted_command() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_flow_active_false_still_blocks_compound() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status && git diff", Some(&s), false);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_flow_active_false_still_blocks_deny() {
    let s = deny_settings();
    let (allowed, msg) = validate("git rebase main", Some(&s), false);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_flow_active_false_still_blocks_redirect() {
    let s = sample_settings();
    let (allowed, msg) = validate("git log > /tmp/out.txt", Some(&s), false);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_flow_active_default_blocks_unlisted() {
    let s = sample_settings();
    let (allowed, msg) = validate("npm test", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_compound_blocked_before_whitelist() {
    let s = sample_settings();
    let (allowed, msg) = validate("git status && git diff", Some(&s), true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

// --- Deny list ---

#[test]
fn test_deny_blocks_matching_command() {
    let s = deny_settings();
    let (allowed, msg) = validate("git rebase main", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_overrides_allow() {
    let s = deny_settings();
    let (allowed, msg) = validate("git checkout feature-branch", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_blocks_force_push() {
    let s = deny_settings();
    let (allowed, msg) = validate("git push --force origin main", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_blocks_hard_reset() {
    let s = deny_settings();
    let (allowed, msg) = validate("git reset --hard HEAD~1", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

#[test]
fn test_deny_allows_non_matching_command() {
    let s = deny_settings();
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_no_settings() {
    let (allowed, msg) = validate("git rebase main", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_empty_deny() {
    let s = json!({"permissions": {"allow": ["Bash(git status)"], "deny": []}});
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_skipped_when_no_deny_key() {
    let s = json!({"permissions": {"allow": ["Bash(git status)"]}});
    let (allowed, msg) = validate("git status", Some(&s), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_deny_runs_before_allow() {
    let s = json!({
        "permissions": {
            "allow": ["Bash(git stash *)"],
            "deny": ["Bash(git stash *)"]
        }
    });
    let (allowed, msg) = validate("git stash save", Some(&s), true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("deny"));
}

// --- Layer 4: structural find -exec/-execdir/-ok/-okdir/-delete block ---
//
// Layer 4 in src/hooks/validate_pretool.rs::validate tokenizes find
// invocations and rejects any of the destructive flag forms
// (`-exec`, `-execdir`, `-ok`, `-okdir`, `-delete`) regardless of
// `settings` content or `flow_active` state. The block fires for
// both with-path forms (`find . -exec rm {} \;`) AND no-path forms
// (`find -exec rm {} \;` — find defaults the path to `.`) because
// tokenization is structural rather than regex-pattern-based.
//
// The tests below pass `None` for settings and `false` for
// flow_active to prove the block fires independently of those
// surfaces — closing the pre-prime upgrade-window gap and the
// outside-FLOW-phase gap that a settings-driven deny would leave
// open.

#[test]
fn test_blocks_find_exec_with_path() {
    let (allowed, msg) = validate("find . -name x -exec rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-exec"));
}

#[test]
fn test_blocks_find_execdir_with_path() {
    let (allowed, msg) = validate("find . -execdir rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-execdir"));
}

#[test]
fn test_blocks_find_ok_with_path() {
    let (allowed, msg) = validate("find . -ok rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-ok"));
}

#[test]
fn test_blocks_find_okdir_with_path() {
    let (allowed, msg) = validate("find . -okdir rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-okdir"));
}

#[test]
fn test_blocks_find_delete_with_path() {
    let (allowed, msg) = validate("find . -name x -delete", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-delete"));
}

// --- Layer 4: no-path bypass shapes ---
//
// `find -exec rm` and `find -delete` (path defaults to `.`) are the
// canonical destructive shapes a regex pattern requiring a non-empty
// path slot would silently pass. Layer 4's structural tokenization
// catches them.

#[test]
fn test_blocks_find_exec_without_path() {
    let (allowed, msg) = validate("find -exec rm /etc/passwd \\;", None, false);
    assert!(
        !allowed,
        "find -exec without path must be blocked; msg={msg:?}"
    );
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-exec"));
}

#[test]
fn test_blocks_find_execdir_without_path() {
    let (allowed, msg) = validate("find -execdir rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-execdir"));
}

#[test]
fn test_blocks_find_ok_without_path() {
    let (allowed, msg) = validate("find -ok rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-ok"));
}

#[test]
fn test_blocks_find_okdir_without_path() {
    let (allowed, msg) = validate("find -okdir rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-okdir"));
}

#[test]
fn test_blocks_find_delete_without_path() {
    let (allowed, msg) = validate("find -delete", None, false);
    assert!(
        !allowed,
        "find -delete without path recursively unlinks cwd; must be blocked; msg={msg:?}"
    );
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-delete"));
}

// --- Layer 4: absolute-path /find variant ---

#[test]
fn test_blocks_absolute_path_find_exec() {
    let (allowed, msg) = validate("/usr/bin/find . -exec rm {} \\;", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("-exec"));
}

// --- Layer 4: safe find invocations pass ---
//
// Read-only find shapes (no destructive flag) must NOT be blocked
// by Layer 4 — they fall through to subsequent layers so the
// whitelist (Layer 9) can permit them via UNIVERSAL_ALLOW's
// `Bash(find *)` allow.

#[test]
fn test_layer4_skips_safe_find() {
    let (allowed, msg) = validate("find . -name foo", None, false);
    assert!(allowed, "safe find must pass Layer 4; msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_layer4_skips_non_find_command() {
    // First token isn't `find` — Layer 4 must not fire even if
    // the command contains `-exec` as a literal arg later.
    let (allowed, _msg) = validate("ls -la -exec", None, false);
    assert!(allowed);
}

// --- Layer 8: structural escape-hatch program/flag block ---
//
// Layer 8 in src/hooks/validate_pretool.rs::validate strips env-var
// prefixes (KEY=VAL ...), strips the path prefix to a basename, and
// matches the basename against the escape-hatch program set from
// `.claude/rules/no-escape-hatches.md` "Canonical Escape-Hatch Shapes"
// with trigger-flag awareness. The block fires regardless of
// `settings` content or `flow_active` state so the protection holds
// during the pre-prime upgrade window AND outside FLOW phases.
//
// Each test passes None for settings and false for flow_active so the
// block is provably independent of those surfaces. Block messages
// must cite `.claude/rules/no-escape-hatches.md` so retrofit drift
// fails the citation contract test.

// Shell-eval direct-form rejections.

#[test]
fn test_blocks_bash_dash_c() {
    let (allowed, msg) = validate("bash -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("bash"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_sh_dash_c() {
    let (allowed, msg) = validate("sh -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_zsh_dash_c() {
    let (allowed, msg) = validate("zsh -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_eval_command() {
    let (allowed, msg) = validate("eval 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

// Command-wrapper direct-form rejections.

#[test]
fn test_blocks_xargs_command() {
    let (allowed, msg) = validate("xargs ls", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_rtk_proxy() {
    let (allowed, msg) = validate("rtk proxy ls", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

// Interpreter-eval direct-form rejections.

#[test]
fn test_blocks_perl_dash_e_lowercase() {
    let (allowed, msg) = validate("perl -e 'print 1'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_perl_dash_e_uppercase() {
    let (allowed, msg) = validate("perl -E 'say 1'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_python_dash_c() {
    let (allowed, msg) = validate("python -c 'print(1)'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_python3_dash_c() {
    let (allowed, msg) = validate("python3 -c 'print(1)'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_ruby_dash_e() {
    let (allowed, msg) = validate("ruby -e 'puts 1'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_node_dash_e() {
    let (allowed, msg) = validate("node -e 'console.log(1)'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_node_dash_p() {
    let (allowed, msg) = validate("node -p '1+1'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

// Network-bridge direct-form rejections.

#[test]
fn test_blocks_nc_command() {
    let (allowed, msg) = validate("nc 1.2.3.4 80", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_ssh_command() {
    let (allowed, msg) = validate("ssh host", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

// Inter-process direct-form rejections.

#[test]
fn test_blocks_tmux_send_keys() {
    let (allowed, msg) = validate("tmux send-keys 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_screen_capital_x() {
    let (allowed, msg) = validate("screen -X stuff cmd", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

// --- Layer 8: indirect-form rejections ---
//
// Glob deny patterns require the exact first-token spelling; these
// indirect shapes (absolute path prefix, env-var prefix, flags
// before the trigger) route around Layer 7's settings-driven check.
// Layer 8's structural tokenization catches them.

#[test]
fn test_blocks_absolute_path_bash_dash_c() {
    let (allowed, msg) = validate("/usr/bin/bash -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_absolute_path_sh_dash_c() {
    let (allowed, msg) = validate("/bin/sh -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_env_prefix_bash_dash_c() {
    let (allowed, msg) = validate("FOO=bar bash -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_multiple_env_prefix_bash_dash_c() {
    let (allowed, msg) = validate("A=1 B=2 bash -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_bash_norc_dash_c() {
    let (allowed, msg) = validate("bash --norc -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_blocks_bash_login_dash_c() {
    let (allowed, msg) = validate("bash --login -c 'ls'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- Layer 8: pass-through cases ---
//
// `bash -n script.sh` (syntax check, no eval) is in UNIVERSAL_ALLOW
// and must pass Layer 8 untouched. `ssh-keygen` has the basename
// `ssh-keygen` rather than `ssh` and must NOT trip the ssh-class
// block — basename matching is exact, not prefix-based.

#[test]
fn test_layer_7_5_passes_bash_dash_n() {
    let (allowed, msg) = validate("bash -n script.sh", None, false);
    assert!(allowed, "bash -n must pass Layer 8; msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_layer_7_5_passes_ssh_keygen() {
    // Pass settings=None so we skip Layer 9's whitelist; the test is
    // that Layer 8 doesn't fire on `ssh-keygen`.
    let (allowed, _msg) = validate("ssh-keygen -t rsa", None, false);
    assert!(
        allowed,
        "ssh-keygen basename must not match ssh-class block"
    );
}

#[test]
fn test_layer_7_5_passes_python_without_dash_c() {
    // `python script.py` is a script execution, not a -c eval — the
    // shell-eval class doesn't apply. Falls through to other layers.
    let (allowed, _msg) = validate("python script.py", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_node_without_eval_flag() {
    let (allowed, _msg) = validate("node script.js", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_perl_script_invocation() {
    let (allowed, _msg) = validate("perl script.pl", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_rtk_subcommand_other_than_proxy() {
    let (allowed, _msg) = validate("rtk discover", None, false);
    assert!(allowed, "rtk subcommands other than proxy must pass");
}

#[test]
fn test_layer_7_5_passes_ruby_script_invocation() {
    // `ruby script.rb` is a script run, not a `-e` eval — falls
    // through to subsequent layers.
    let (allowed, _msg) = validate("ruby script.rb", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_tmux_ls() {
    // `tmux ls` lists sessions — not the `send-keys` injection
    // shape, so Layer 8 must let it through.
    let (allowed, _msg) = validate("tmux ls", None, false);
    assert!(allowed, "tmux without send-keys subcommand must pass");
}

#[test]
fn test_layer_7_5_passes_screen_ls() {
    // `screen -ls` lists sessions — not the `-X` stuff-key shape, so
    // Layer 8 must let it through.
    let (allowed, _msg) = validate("screen -ls", None, false);
    assert!(allowed, "screen without -X flag must pass");
}

#[test]
fn test_layer_7_5_passes_bare_env_assignment() {
    // A `KEY=VAL` assignment with no trailing whitespace and no
    // following command is structurally an env-var set;
    // `strip_env_prefix` does not strip the final segment because
    // there is no whitespace boundary proving a following command
    // exists. The tokenized basename is `KEY=VAL`, which matches no
    // escape-hatch program — Layer 8 returns None and the call
    // passes through.
    let (allowed, _msg) = validate("FOO=BAR", None, false);
    assert!(allowed);
}

// --- Layer 8: combined-flag scan (adversarial regression) ---
//
// `bash -lc 'cmd'` packs `-l` (login) and `-c` (eval) into a single
// token. A literal `rest.contains(&"-c")` check matches only the
// literal `-c` token, missing the combined-flag shape. The
// `has_flag_char` helper iterates each short-flag token character-
// by-character so any token starting with `-` (but not `--`) that
// contains the trigger character matches.

#[test]
fn test_layer_7_5_blocks_bash_dash_lc() {
    let (allowed, msg) = validate("bash -lc 'echo hi'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_layer_7_5_blocks_bash_dash_ic() {
    let (allowed, msg) = validate("bash -ic 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_bash_dash_xc() {
    let (allowed, msg) = validate("bash -xc 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_env_prefix_bash_dash_lc() {
    // Compounds the env-prefix strip with the combined-flag scan.
    let (allowed, msg) = validate("FOO=bar bash -lc 'echo hi'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_passes_bash_dash_n_long() {
    // `bash --noprofile -n script.sh` — long flag, no eval trigger.
    // Must pass.
    let (allowed, _msg) = validate("bash --noprofile -n script.sh", None, false);
    assert!(allowed);
}

// --- Layer 8: wrapper-launcher strip (adversarial regression) ---
//
// `env`, `time`, `nice`, `nohup`, `taskset`, `ionice` wrap another
// command. The `strip_wrapper_launchers` helper consumes the
// wrapper token so the effective program reaches the basename
// check.

#[test]
fn test_layer_7_5_blocks_env_bash_dash_c() {
    let (allowed, msg) = validate("env bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_env_with_kv_bash_dash_c() {
    // env's KEY=VAL args land between the wrapper and the wrapped
    // program. After strip_wrapper_launchers consumes `env`,
    // strip_env_prefix consumes the KEY=VAL, exposing `bash -c`.
    let (allowed, msg) = validate("env FOO=bar bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_time_bash_dash_c() {
    let (allowed, msg) = validate("time bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_nice_python_dash_c() {
    let (allowed, msg) = validate("nice python -c 'print(1)'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_nohup_bash_dash_c() {
    let (allowed, msg) = validate("nohup bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_taskset_bash_dash_c() {
    let (allowed, msg) = validate("taskset bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_ionice_bash_dash_c() {
    let (allowed, msg) = validate("ionice bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_blocks_absolute_path_env_bash_dash_c() {
    let (allowed, msg) = validate("/usr/bin/env bash -c 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_layer_7_5_passes_env_with_only_wrapper() {
    // `env` alone with no following command — wrapper-launcher
    // returns empty. The next token check sees nothing, the
    // `first` extraction fails, and Layer 8 returns None.
    let (allowed, _msg) = validate("env", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_time_alone() {
    let (allowed, _msg) = validate("time", None, false);
    assert!(allowed);
}

// --- Layer 8: additional interpreter-eval programs ---
//
// osascript (macOS AppleScript), tclsh (Tcl), and lua all evaluate
// strings passed via -e/-c flags and have builtins that shell out
// (`do shell script`, `exec`, `os.execute`). Same interpreter-eval
// class as perl/python/ruby/node; added to the escape-hatch program
// set during the Review fix sweep.

#[test]
fn test_layer_7_5_blocks_osascript_dash_e() {
    let (allowed, msg) = validate("osascript -e 'do shell script \"id\"'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("osascript"));
    assert!(msg.contains("no-escape-hatches.md"));
}

#[test]
fn test_layer_7_5_blocks_tclsh_dash_c() {
    let (allowed, msg) = validate("tclsh -c 'exec id'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("tclsh"));
}

#[test]
fn test_layer_7_5_blocks_lua_dash_e() {
    let (allowed, msg) = validate("lua -e 'os.execute(\"id\")'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("lua"));
}

#[test]
fn test_layer_7_5_passes_osascript_script_invocation() {
    // `osascript script.scpt` runs an AppleScript file; no -e flag.
    let (allowed, _msg) = validate("osascript script.scpt", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_tclsh_script_invocation() {
    let (allowed, _msg) = validate("tclsh script.tcl", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_lua_script_invocation() {
    let (allowed, _msg) = validate("lua script.lua", None, false);
    assert!(allowed);
}

// --- Layer 8: tmux with global flags (adversarial regression) ---
//
// `tmux send-keys` was previously caught only when send-keys was
// the first arg token. Global tmux flags (`-L socket`, `-S path`,
// `-f config`, `-v`) before the subcommand shifted send-keys past
// the `rest.first()` check. Fixed by switching to `rest.contains`.

#[test]
fn test_layer_7_5_blocks_tmux_with_socket_flag() {
    let (allowed, msg) = validate("tmux -L mysocket send-keys 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
    assert!(msg.contains("send-keys"));
}

#[test]
fn test_layer_7_5_blocks_tmux_with_config_flag() {
    let (allowed, msg) = validate("tmux -f /tmp/cfg send-keys 'cmd'", None, false);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

// --- Layer 8: block message sanctioned-alternative content ---

#[test]
fn test_bash_block_message_names_sanctioned_alternative() {
    let (_, msg) = validate("bash -c 'ls'", None, false);
    assert!(
        msg.contains("separate Bash"),
        "bash -c block must name the sanctioned alternative; msg={msg:?}"
    );
}

#[test]
fn test_python_block_message_names_sanctioned_alternative() {
    let (_, msg) = validate("python -c 'x'", None, false);
    assert!(
        msg.contains("Read tool") || msg.contains("Write tool"),
        "python -c block must name the sanctioned alternative; msg={msg:?}"
    );
}

#[test]
fn test_xargs_block_message_names_sanctioned_alternative() {
    let (_, msg) = validate("xargs ls", None, false);
    assert!(
        msg.contains("separate Bash"),
        "xargs block must name the sanctioned alternative; msg={msg:?}"
    );
}

#[test]
fn test_ssh_block_message_names_sanctioned_alternative() {
    let (_, msg) = validate("ssh host", None, false);
    assert!(
        msg.contains("ssh wrapper") || msg.contains("approved ssh"),
        "ssh block must name the sanctioned alternative; msg={msg:?}"
    );
}

// --- Read-only file commands pass with active flow + standard allow list ---
//
// UNIVERSAL_ALLOW carries `Bash(cat *)`, `Bash(grep *)`, `Bash(find *)`,
// `Bash(ls *)`, `Bash(rg *)`, `Bash(head *)`, `Bash(tail *)` — so a primed
// target project allows these read-only commands when a flow is active.
// The synthetic settings below mirror the relevant subset of the
// universal allow list and assert each command falls through every
// preceding layer (compound, redirection, exec, restore, git diff,
// deny) into the whitelist check, which then permits the call.

fn read_only_allow_settings() -> Value {
    json!({
        "permissions": {
            "allow": [
                "Bash(cat *)",
                "Bash(grep *)",
                "Bash(find *)",
                "Bash(ls *)",
                "Bash(rg *)",
                "Bash(head *)",
                "Bash(tail *)",
            ],
            "deny": []
        }
    })
}

#[test]
fn test_allows_cat_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("cat foo", Some(&s), true);
    assert!(allowed, "cat should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_head_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("head -n 5 foo", Some(&s), true);
    assert!(allowed, "head -n 5 foo should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_tail_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("tail foo", Some(&s), true);
    assert!(allowed, "tail should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_ls_bare_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("ls", Some(&s), true);
    // Bare `ls` (no args) does not match `Bash(ls *)` because the
    // glob requires at least a trailing space + char. The whitelist
    // check rejects it. This documents the expected behavior so a
    // future widening of the allow pattern is a deliberate decision.
    assert!(!allowed, "bare ls should still hit whitelist rejection");
    assert!(msg.contains("not in allow list"));
}

#[test]
fn test_allows_ls_la_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("ls -la", Some(&s), true);
    assert!(allowed, "ls -la should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_grep_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("grep pat file", Some(&s), true);
    assert!(allowed, "grep should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_rg_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("rg pat", Some(&s), true);
    assert!(allowed, "rg should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_find_simple_with_active_flow() {
    let s = read_only_allow_settings();
    let (allowed, msg) = validate("find . -name x", Some(&s), true);
    assert!(allowed, "find . -name x should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

// --- Redirect blocking ---

#[test]
fn test_blocks_redirect_output() {
    let (allowed, msg) = validate("git show HEAD:file.py > /tmp/out.py", None, true);
    assert!(!allowed);
    assert!(msg.contains("Read tool"));
    assert!(msg.contains("Write tool"));
}

#[test]
fn test_blocks_redirect_append() {
    let (allowed, msg) = validate("git log >> /tmp/out.txt", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_redirect_stderr() {
    let (allowed, msg) = validate("git status 2> /tmp/err.txt", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_redirect_no_space() {
    let (allowed, msg) = validate("git show HEAD:file.py>/tmp/out.py", None, true);
    assert!(!allowed);
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_allows_no_redirect() {
    let (allowed, msg) = validate("git diff --diff-filter=M", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_allows_arrow_in_flag() {
    let (allowed, msg) = validate("git log --format=>%s", None, true);
    assert!(allowed);
    assert!(msg.is_empty());
}

// --- FD-redirect pass-through ---
//
// `2>&1`, `>&2`, `2>&-`, `2>&1 1>&2` are file-descriptor redirect
// forms — the `&` is the redirect-target marker, not the bash
// backgrounding operator. These must pass Layer 1 (compound-op
// detector) and Layer 2 (redirect detector) so common test commands
// like `cargo test 2>&1` and `bin/flow ci 2>&1` are not falsely
// blocked. Plain `&` backgrounding (`cmd & wait`) and bare `&` at
// command start (`&1 cmd`) still block.

#[test]
fn test_allows_fd_redirect_2_to_1() {
    let (allowed, msg) = validate("cargo test 2>&1", None, true);
    assert!(allowed, "cargo test 2>&1 should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_fd_redirect_to_stderr() {
    let (allowed, msg) = validate("echo oops >&2", None, true);
    assert!(allowed, "echo oops >&2 should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_fd_redirect_close() {
    let (allowed, msg) = validate("cmd 2>&-", None, true);
    assert!(allowed, "cmd 2>&- should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_fd_redirect_swap() {
    let (allowed, msg) = validate("cmd 2>&1 1>&2", None, true);
    assert!(allowed, "cmd 2>&1 1>&2 should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_allows_quoted_command_with_fd_redirect() {
    let (allowed, msg) = validate("echo 'cmd 2>&1'", None, true);
    assert!(allowed, "quoted 'cmd 2>&1' should pass — got msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_blocks_compound_with_fd_redirect_still_blocks_pipe() {
    // `2>&1` itself passes, but the `|` later in the line still
    // blocks at Layer 1's compound-op gate.
    let (allowed, msg) = validate("cmd 2>&1 | grep foo", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_blocks_bare_ampersand_backgrounding() {
    // `cmd & wait` — bare `&` between commands is backgrounding,
    // not FD-redirect. Must still block.
    let (allowed, msg) = validate("cmd & wait", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_blocks_leading_ampersand_defensive() {
    // `&1 cmd` — `&` at start with no preceding `>`. Not a valid
    // FD-redirect form; defensively block as backgrounding-shaped.
    let (allowed, msg) = validate("&1 cmd", None, true);
    assert!(!allowed);
    assert!(msg.contains("Compound commands"));
}

#[test]
fn test_blocks_amp_redirect_to_file_with_space() {
    // `cmd >& outfile` is bash file-redirect syntax (redirects
    // both stdout and stderr to a file named outfile). The
    // `is_fd_redirect_at` helper must NOT carve this out — Layer 2
    // (redirect detector) must still see the `>` as a structural
    // redirect operator. Without the digit/`-`-after-`&`
    // constraint, this shape silently bypassed both gates.
    let (allowed, msg) = validate("cmd >& outfile", None, true);
    assert!(
        !allowed,
        "`cmd >& outfile` is a file-redirect that should still block — got msg={msg:?}"
    );
}

#[test]
fn test_blocks_amp_redirect_to_relative_file() {
    let (allowed, msg) = validate("echo hello >& output.log", None, true);
    assert!(
        !allowed,
        "`echo hello >& output.log` is a file-redirect that should still block — got msg={msg:?}"
    );
}

#[test]
fn test_blocks_amp_redirect_with_letter_target() {
    // `>&letter` (no space) is also bash file-redirect — `letter`
    // is not a digit or `-`, so it is not a valid FD target.
    let (allowed, msg) = validate("cmd >&letter", None, true);
    assert!(
        !allowed,
        "`cmd >&letter` is a file-redirect that should still block — got msg={msg:?}"
    );
}

#[test]
fn test_blocks_amp_redirect_at_input_start() {
    // `>& outfile` at idx=0 is still file-redirect syntax. The
    // helper's `>` arm fires at idx=0 (next=`&`, after_amp=` ` →
    // not a digit/`-`), so it correctly returns false and Layer 2
    // catches the `>`.
    let (allowed, _msg) = validate(">& outfile", None, true);
    assert!(
        !allowed,
        "`>& outfile` at input start is still a file redirect"
    );
}

// --- run_in_background blocking ---

#[test]
fn test_blocks_background_bin_flow_ci_outside_flow() {
    let msg = should_block_background("bin/flow ci", false);
    assert!(msg.is_some());
    let text = msg.unwrap();
    assert!(text.contains("bin/flow"));
    assert!(text.contains("bin/ci"));
}

#[test]
fn test_blocks_background_bin_flow_ci_with_args_outside_flow() {
    let msg = should_block_background("bin/flow ci --retry 3", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bin_ci_outside_flow() {
    let msg = should_block_background("bin/ci", false);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("bin/ci"));
}

#[test]
fn test_blocks_background_absolute_bin_flow_ci_outside_flow() {
    let msg = should_block_background("/Users/ben/code/flow/bin/flow ci", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_absolute_bin_ci_outside_flow() {
    let msg = should_block_background("/Users/ben/code/flow/bin/ci", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bin_flow_finalize_commit() {
    let msg = should_block_background("bin/flow finalize-commit main", false);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("bin/flow"));
}

#[test]
fn test_blocks_background_bin_flow_phase_transition() {
    let msg = should_block_background("bin/flow phase-transition --action complete", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_absolute_bin_flow_finalize_commit() {
    let msg = should_block_background("/Users/ben/code/flow/bin/flow finalize-commit main", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_bare_bin_flow() {
    let msg = should_block_background("bin/flow", false);
    assert!(msg.is_some());
}

#[test]
fn test_blocks_background_any_command_inside_flow() {
    let msg = should_block_background("echo hi", true);
    assert!(msg.is_some());
    assert!(msg.unwrap().contains("FLOW phase"));
}

#[test]
fn test_allows_background_non_flow_outside_flow() {
    let msg = should_block_background("echo hi", false);
    assert!(msg.is_none());
}

#[test]
fn test_does_not_false_positive_on_commands_containing_flow() {
    assert!(should_block_background("npm run ci", false).is_none());
    assert!(should_block_background("git commit", false).is_none());
    assert!(should_block_background("npm run flow", false).is_none());
}

#[test]
fn test_is_flow_command_empty_returns_false() {
    assert!(should_block_background("", false).is_none());
}

#[test]
fn test_is_flow_command_whitespace_only_returns_false() {
    assert!(should_block_background("   \t", false).is_none());
}

// --- is_bg_truthy: defensive JSON type handling (subprocess tests) ---
//
// `is_bg_truthy` is a private helper called inside `run()` against the
// `tool_input.run_in_background` field. We drive it by spawning the
// compiled binary and feeding JSON via stdin:
//   - When `is_bg_truthy` returns true → `should_block_background` runs
//     against `command = "bin/flow ci"` and the process exits 2 with a
//     block message on stderr.
//   - When `is_bg_truthy` returns false → the background path is skipped
//     and `validate("bin/flow ci", ...)` allows the command → exit 0.
// Command `bin/flow ci` is deliberately chosen: it's on FLOW's own
// whitelist (allowed by `validate`) AND it's a CI-tier command that
// `should_block_background` always blocks when `is_bg_truthy` is true
// (regardless of flow_active).

fn run_hook_with_bg(bg: Value) -> (i32, String, String) {
    // Isolate the child's cwd from any host-environment FLOW state.
    // Any active Code-phase flow on the host machine has
    // `current_phase=flow-code, status=in_progress` in its state
    // file; a `bin/flow ci` invocation rooted in that worktree
    // would trip Layer 11 and produce an unexpected block. These
    // tests only exercise the `is_bg_truthy` decision — the cwd's
    // FLOW state is irrelevant to that decision but reaches
    // Layer 11 when bg is falsy and the bg check falls through.
    let isolation = tempfile::tempdir().expect("tempdir");
    let input = json!({
        "tool_input": {
            "command": "bin/flow ci",
            "run_in_background": bg,
        }
    });
    let output = crate::common::spawn_hook(
        "validate-pretool",
        isolation.path(),
        serde_json::to_string(&input).unwrap().as_bytes(),
        &[("HOME", isolation.path().to_str().unwrap())],
    );
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn is_bg_truthy_bool_true_blocks() {
    let (code, _stdout, stderr) = run_hook_with_bg(json!(true));
    assert_eq!(code, 2, "bool true should block; stderr={stderr}");
    assert!(stderr.contains("bin/flow"));
}

#[test]
fn is_bg_truthy_bool_false_allows() {
    let (code, _stdout, stderr) = run_hook_with_bg(json!(false));
    assert_eq!(code, 0, "bool false should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_true_case_insensitive_blocks() {
    let (code, _, stderr) = run_hook_with_bg(json!("True"));
    assert_eq!(code, 2, "\"True\" should block; stderr={stderr}");
    let (code, _, stderr) = run_hook_with_bg(json!("TRUE"));
    assert_eq!(code, 2, "\"TRUE\" should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_one_blocks() {
    let (code, _, stderr) = run_hook_with_bg(json!("1"));
    assert_eq!(code, 2, "\"1\" should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_string_other_allows() {
    // Non-truthy strings: "false", "0", "yes", "", "foreground"
    for s in &["false", "0", "yes", "", "foreground"] {
        let (code, _, stderr) = run_hook_with_bg(json!(s));
        assert_eq!(
            code, 0,
            "string {s:?} should not block; got exit={code} stderr={stderr}"
        );
    }
}

#[test]
fn is_bg_truthy_integer_nonzero_blocks() {
    for n in &[1_i64, 42, -1] {
        let (code, _, stderr) = run_hook_with_bg(json!(n));
        assert_eq!(
            code, 2,
            "integer {n} should block; got exit={code} stderr={stderr}"
        );
    }
}

#[test]
fn is_bg_truthy_integer_zero_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!(0_i64));
    assert_eq!(code, 0, "integer 0 should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_f64_nonzero_blocks() {
    // serde_json::Number stores float literals as Float variant; as_i64
    // returns None so evaluation falls through to the as_f64 arm.
    let (code, _, stderr) = run_hook_with_bg(json!(1.5_f64));
    assert_eq!(code, 2, "f64 1.5 should block; stderr={stderr}");
}

#[test]
fn is_bg_truthy_f64_zero_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!(0.0_f64));
    assert_eq!(code, 0, "f64 0.0 should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_null_allows() {
    let (code, _, stderr) = run_hook_with_bg(Value::Null);
    assert_eq!(code, 0, "null should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_array_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!([true, 1]));
    assert_eq!(code, 0, "array should allow; stderr={stderr}");
}

#[test]
fn is_bg_truthy_object_allows() {
    let (code, _, stderr) = run_hook_with_bg(json!({"x": 1}));
    assert_eq!(code, 0, "object should allow; stderr={stderr}");
}

// --- run() branch coverage via subprocess ---
//
// Each test drives a distinct branch of `run()` that cannot be reached
// through the library surface: stdin parsing, settings/project-root
// discovery, Agent-tool dispatch, and the validate() exit-2 fall-through.

fn run_hook_with_input(input: &str, cwd: Option<&std::path::Path>) -> (i32, String, String) {
    run_hook_with_input_and_home(input, cwd, None)
}

/// Subprocess test helper that lets the caller override the child
/// process's HOME env var. Used by tests that include a
/// `transcript_path` in the hook input — the walker validates the
/// path is rooted under `<home>/.claude/projects/`, so HOME must
/// point at the tempdir that holds the transcript fixture for the
/// validator to accept the path. Tests that don't pass a
/// transcript_path can continue using `run_hook_with_input` and
/// inherit the test runner's HOME unchanged.
fn run_hook_with_input_and_home(
    input: &str,
    cwd: Option<&std::path::Path>,
    home: Option<&std::path::Path>,
) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "validate-pretool"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(h) = home {
        cmd.env("HOME", h);
    }
    let mut child = cmd.spawn().expect("spawn flow-rs");
    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Build a JSONL transcript line representing an assistant turn
/// with a Skill tool_use whose `input.skill` is the given name. Used
/// by the skill-commit carve-out tests to build controlled
/// transcript fixtures that drive the walker's
/// `most_recent_skill_since_user` predicate.
fn assistant_skill_jsonl(skill: &str) -> String {
    format!(
        "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"tool_use\",\"name\":\"Skill\",\"input\":{{\"skill\":\"{}\"}}}}]}}}}\n",
        skill
    )
}

/// Build a JSONL user-turn line with the given content string. The
/// walker's `most_recent_skill_since_user` stops at the most recent
/// user turn going backward, so a user turn after a Skill call
/// invalidates the walker's view of that Skill.
fn user_jsonl(content: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{}\"}}}}\n",
        content
    )
}

/// Covers `None => exit(0)` in `match read_hook_input()` — non-JSON
/// stdin makes `read_hook_input` return None.
#[test]
fn run_rejects_malformed_stdin_and_exits_zero() {
    let (code, _, _) = run_hook_with_input("not valid json", None);
    assert_eq!(code, 0, "malformed stdin must exit 0");
}

/// Covers the `else { None }` branch of `branch = if settings.is_some()`
/// and the `_ => false` flow_active arm: running from a cwd with no
/// .claude/settings.json makes `find_settings_and_root` return
/// `(None, None)`, so settings.is_none() and the (&branch, &main_root)
/// match both take the wildcard arm.
#[test]
fn run_without_settings_falls_through_branch_and_main_root() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "git status"}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 0, "allowed command with no settings must exit 0");
}

/// Covers the `should_block_background(...)` fall-through when the
/// command is NOT a flow command and flow_active is false:
/// is_bg_truthy=true, should_block_background returns None, so execution
/// falls past the background block and continues.
#[test]
fn run_with_bg_true_non_flow_command_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "git status", "run_in_background": true}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(
        code, 0,
        "bg=true on non-flow command outside flow must fall through"
    );
}

/// Covers the Agent-tool allow path: empty command + !flow_active →
/// validate_agent returns (true, ""), so we hit `exit(0)` inside the
/// `if command.is_empty()` block.
#[test]
fn run_agent_path_allowed_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {}}"#;
    let (code, _, _) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 0, "empty command outside flow must exit 0");
}

/// Covers the validate()-rejected exit-2 path: `git restore .` is
/// blocked at Layer 5 regardless of flow-active state, so validate()
/// returns (false, msg) and run() eprintlns the message and exits 2.
#[test]
fn run_validate_rejection_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    let input = r#"{"tool_input": {"command": "git restore ."}}"#;
    let (code, _, stderr) = run_hook_with_input(input, Some(dir.path()));
    assert_eq!(code, 2, "git restore . must be blocked; stderr={stderr}");
    assert!(stderr.contains("BLOCKED"));
}

/// Covers the Agent-tool block path (eprintln + exit 2) when
/// flow_active is true. Builds a fake worktree layout under a tempdir:
///   root/.claude/settings.json              — satisfies find_settings_and_root
///   root/.flow-states/<branch>/state.json   — makes is_flow_active return true
///   root/.worktrees/<branch>/.git           — makes detect_branch_from_path
///                                             identify the branch from cwd
/// Then spawns the hook with cwd=root/.worktrees/<branch>/ and a
/// general-purpose subagent payload, which must exit 2 with a BLOCKED
/// message.
#[test]
fn run_agent_path_blocked_exits_two_when_flow_active() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();

    let claude_dir = root_path.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let branch_dir = root_path.join(".flow-states").join("feat");
    std::fs::create_dir_all(&branch_dir).unwrap();
    std::fs::write(branch_dir.join("state.json"), "{}").unwrap();

    let worktree = root_path.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/feat").unwrap();

    let input = r#"{"tool_input": {"subagent_type": "general-purpose"}}"#;
    let (code, _, stderr) = run_hook_with_input(input, Some(&worktree));
    assert_eq!(
        code, 2,
        "general-purpose agent during active flow must exit 2; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("general-purpose"));
}

// --- Agent validation ---

#[test]
fn test_validate_agent_blocks_general_purpose_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("general-purpose"), true);
    assert!(!allowed);
    assert!(msg.contains("general-purpose"));
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_blocks_absent_type_when_flow_active() {
    let (allowed, msg) = validate_agent(None, true);
    assert!(!allowed);
    assert!(msg.contains("general-purpose"));
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_allows_flow_namespace_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("flow:ci-fixer"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_explore_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("Explore"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_plan_when_flow_active() {
    let (allowed, msg) = validate_agent(Some("Plan"), true);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_general_purpose_when_no_flow() {
    let (allowed, msg) = validate_agent(Some("general-purpose"), false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_allows_absent_type_when_no_flow() {
    let (allowed, msg) = validate_agent(None, false);
    assert!(allowed);
    assert!(msg.is_empty());
}

#[test]
fn test_validate_agent_blocks_case_variants_when_flow_active() {
    let (allowed, _) = validate_agent(Some("General-Purpose"), true);
    assert!(!allowed);
    let (allowed, _) = validate_agent(Some("GENERAL-PURPOSE"), true);
    assert!(!allowed);
}

#[test]
fn test_validate_agent_blocks_empty_string_when_flow_active() {
    let (allowed, msg) = validate_agent(Some(""), true);
    assert!(!allowed);
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_validate_agent_blocks_whitespace_padded_when_flow_active() {
    let (allowed, _) = validate_agent(Some(" general-purpose "), true);
    assert!(!allowed);
}

// --- quote_aware_scan ---

#[test]
fn test_allows_pipe_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'describes | operator'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "pipe inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_pipe_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"describes | operator\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "pipe inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_semicolon_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a; b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "semicolon inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_semicolon_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"a; b\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "semicolon inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_ampersand_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'foo && bar'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "&& inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_ampersand_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"foo && bar\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "&& inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_or_operator_in_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a || b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "|| inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_redirect_char_in_single_quoted_arg() {
    let cmd = "bin/flow add-finding --reason 'a > b'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "> inside single quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_allows_redirect_char_in_double_quoted_arg() {
    let cmd = "bin/flow add-finding --reason \"a > b\"";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "> inside double quotes should be inert; got: {msg}"
    );
}

#[test]
fn test_still_blocks_unquoted_pipe() {
    let (allowed, msg) = validate("rg foo src | head", None, true);
    assert!(!allowed, "unquoted | must still be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_still_blocks_unquoted_compound_and() {
    let (allowed, msg) = validate("cd foo && git status", None, true);
    assert!(!allowed, "unquoted && must still be blocked");
    assert!(msg.contains("Compound") || msg.contains("&&"));
}

#[test]
fn test_still_blocks_unquoted_semicolon() {
    let (allowed, msg) = validate("bin/ci; echo done", None, true);
    assert!(!allowed, "unquoted ; must still be blocked");
    assert!(msg.contains("Compound") || msg.contains(";"));
}

#[test]
fn test_still_blocks_unquoted_redirect() {
    let (allowed, msg) = validate("git log > /tmp/out", None, true);
    assert!(!allowed, "unquoted > must still be blocked");
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_blocks_operator_after_closing_quote() {
    let (allowed, msg) = validate("echo 'foo' | grep bar", None, true);
    assert!(!allowed, "| after closed quote must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_unclosed_single_quote_with_operator() {
    let (allowed, msg) = validate("echo 'foo | bar", None, true);
    assert!(!allowed, "unclosed single quote must be blocked");
    assert!(
        msg.to_lowercase().contains("unclosed"),
        "error message should name the unclosed-quote case; got: {msg}"
    );
}

#[test]
fn test_blocks_unclosed_double_quote_with_operator() {
    let (allowed, msg) = validate("echo \"foo | bar", None, true);
    assert!(!allowed, "unclosed double quote must be blocked");
    assert!(
        msg.to_lowercase().contains("unclosed"),
        "error message should name the unclosed-quote case; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_pipe_outside_quotes() {
    let (allowed, msg) = validate("echo foo\\|bar", None, true);
    assert!(allowed, "backslash-escaped | must be inert; got: {msg}");
}

#[test]
fn test_allows_mixed_quotes_with_operators() {
    let (allowed, msg) = validate("echo 'a|b' \"c;d\"", None, true);
    assert!(
        allowed,
        "mixed quotes with operators must be inert; got: {msg}"
    );
}

#[test]
fn test_blocks_dollar_paren_command_substitution() {
    let (allowed, msg) = validate("echo $(date)", None, true);
    assert!(!allowed, "unquoted $() must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_dollar_paren_inside_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"the $(cmd) pattern\"", None, true);
    assert!(
        !allowed,
        "$() inside double quotes must be blocked — bash expands it; got: {msg}"
    );
}

#[test]
fn test_blocks_backtick_command_substitution() {
    let (allowed, msg) = validate("echo `date`", None, true);
    assert!(!allowed, "unquoted backtick must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_backtick_inside_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"look: `date`\"", None, true);
    assert!(
        !allowed,
        "backtick inside double quotes must be blocked — bash expands it; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_double_quote_inside_double_quoted_arg() {
    let cmd = r#"echo "hello \"world\"""#;
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "escaped double quote inside double-quoted arg must be literal; got: {msg}"
    );
}

#[test]
fn test_allows_escaped_redirect_inside_double_quoted_arg() {
    let cmd = r#"echo "result \> output""#;
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "escaped redirect char inside double-quoted arg must be literal; got: {msg}"
    );
}

#[test]
fn test_allows_dollar_paren_inside_single_quoted_arg() {
    let cmd = "echo 'literal $(cmd) text'";
    let (allowed, msg) = validate(cmd, None, true);
    assert!(
        allowed,
        "$() inside single quotes must be inert; got: {msg}"
    );
}

#[test]
fn test_allows_backtick_inside_single_quoted_arg() {
    let (allowed, msg) = validate("echo 'look: `tick`'", None, true);
    assert!(
        allowed,
        "backtick inside single quotes must be inert; got: {msg}"
    );
}

#[test]
fn test_allows_quoted_arg_with_redirect_char_after_equals() {
    let (allowed, msg) = validate("git log --format=\"%s > %h\"", None, true);
    assert!(
        allowed,
        "> inside a double-quoted format string must be inert; got: {msg}"
    );
}

// --- adversarial_scan_gaps ---

#[test]
fn test_blocks_input_redirect() {
    let (allowed, msg) = validate("python3 < /etc/passwd", None, true);
    assert!(!allowed, "input redirect must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_here_string() {
    let (allowed, msg) = validate("python3 <<< 'code'", None, true);
    assert!(!allowed, "here-string must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_heredoc() {
    let (allowed, msg) = validate("python3 <<EOF\ncode\nEOF", None, true);
    assert!(!allowed, "heredoc must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_process_substitution_input() {
    let (allowed, msg) = validate("diff <(echo a) <(echo b)", None, true);
    assert!(!allowed, "input process substitution must be blocked");
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_trailing_ampersand_background() {
    let (allowed, msg) = validate("sleep 100 &", None, true);
    assert!(
        !allowed,
        "trailing & background operator must be blocked; got: {msg}"
    );
    assert!(msg.contains("BLOCKED"));
}

#[test]
fn test_blocks_double_dash_redirect() {
    let (allowed, msg) = validate("echo foo-->/tmp/out", None, true);
    assert!(
        !allowed,
        "foo-->/tmp/out must be blocked — the dash carve-out was a bypass vector; got: {msg}"
    );
    assert!(msg.to_lowercase().contains("redirection"));
}

#[test]
fn test_allows_input_redirect_char_in_single_quoted_arg() {
    let (allowed, msg) = validate("echo 'hello <world>'", None, true);
    assert!(allowed, "< inside single quotes must be inert; got: {msg}");
}

#[test]
fn test_allows_input_redirect_char_in_double_quoted_arg() {
    let (allowed, msg) = validate("echo \"hello <world>\"", None, true);
    assert!(allowed, "< inside double quotes must be inert; got: {msg}");
}

#[test]
fn test_allows_ampersand_in_flag_name() {
    let (allowed, msg) = validate("mysql -u root -p'p&w0rd'", None, true);
    assert!(allowed, "& inside single quotes must be inert; got: {msg}");
}

// --- commit_on_integration_branch ---
//
// Layer 10: block direct commit invocations when the hook's effective
// cwd resolves to the integration branch (the value `default_branch_in`
// returns — `main` for the test fixtures below, since no remote HEAD is
// configured and the helper falls back to `"main"`).
//
// Test naming follows a `t<N>_<description>` convention where N is a
// logical group identifier (NOT sequential):
//   - t1, t5, t6           — basic git commit blocking (Task 1)
//   - t2, t3, t4, t14      — feature branch and non-commit allow paths
//                            (Task 3); t4 covers staging integration
//   - t9-t13, t21          — bin/flow finalize-commit recognition and
//                            sibling subcommand allow (Task 5+6),
//                            unknown launcher boundary (Task 6 follow-up)
//   - t7, t8, t15, t16,
//     t23, t24, t25        — adversarial bypasses (Task 7+8): -c k=v,
//                            -C path, quoted command, bash/sh -c,
//                            empty -c/-C values
//   - t17-t20              — documented v1 boundaries (Task 9):
//                            detached HEAD, non-git, alias, xargs
//   - t26                  — bin/flow flag-skip bypass (Review)
//
// The fixture pattern mirrors the existing `run_agent_path_blocked_*`
// tests: `tempfile::tempdir()` + `canonicalize()` per
// `.claude/rules/testing-gotchas.md` "macOS Subprocess Path
// Canonicalization", `git init --initial-branch <name>`, configure
// identity, and a single empty commit so `git branch --show-current`
// returns the named branch.

/// Initialize a tempdir as a git repo on the named branch, with a
/// single empty commit so `git branch --show-current` returns the
/// branch name. Returns the `TempDir` (drop-on-cleanup) and the
/// canonical root path the test must use as cwd and in any
/// `tool_input` paths it builds.
fn setup_repo_on_branch(branch: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let init = Command::new("git")
        .args(["init", "--initial-branch", branch])
        .current_dir(&root)
        .output()
        .expect("git init");
    assert!(init.status.success(), "git init failed: {init:?}");
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(&root)
        .output()
        .expect("git config email");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&root)
        .output()
        .expect("git config name");
    let commit = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output()
        .expect("git commit init");
    assert!(
        commit.status.success(),
        "empty init commit failed: {commit:?}"
    );
    // Synthesize refs/remotes/origin/HEAD pointing at `main` so
    // `git::default_branch_in` resolves cleanly. Validate_pretool
    // Layer 9 compares the dequoted branch arg against the
    // integration branch returned by default_branch_in.
    let _ = Command::new("git")
        .args(["update-ref", "refs/remotes/origin/main", "HEAD"])
        .current_dir(&root)
        .output();
    let _ = Command::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ])
        .current_dir(&root)
        .output();
    (dir, root)
}

// --- run_impl_main cwd-seam contract (Task 4) ---

/// The validate-pretool decision logic lives in a private
/// `run_impl_main(hook_input, cwd)` core; `run()` resolves the cwd and
/// delegates. This guards the delegation contract: the core's outcome
/// is a function of the cwd it receives. A regression that dropped the
/// cwd thread (run_impl_main ignoring its cwd parameter) would make the
/// Layer 10 commit gate stop depending on the working directory's
/// branch — this test trips on that by asserting the same input
/// produces opposite outcomes under two different cwds.
///
/// This is a delegation-contract test for a behavior-preserving
/// extraction, so it passes both before and after the refactor (per
/// `.claude/rules/skill-authoring.md` "Delegation Path Tests Need No
/// Migration"). It is not a TDD-red test — there is no new behavior to
/// red, only a preserved delegation path.
#[test]
fn validate_pretool_run_impl_main_accepts_cwd() {
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;

    // cwd on the integration branch → Layer 10 commit gate fires.
    let (_dir_main, root_main) = setup_repo_on_branch("main");
    let (code_main, _o, stderr_main) = run_hook_with_input(input, Some(&root_main));
    assert_eq!(
        code_main, 2,
        "core must consume cwd: git commit with cwd on main blocks; stderr={stderr_main}"
    );
    assert!(stderr_main.contains("BLOCKED"));

    // Same input, cwd on a feature branch → Layer 10 does not fire.
    // Opposite outcome under a different cwd proves the threaded cwd is
    // the discriminator the core acts on.
    let (_dir_feat, root_feat) = setup_repo_on_branch("feat-x");
    let (code_feat, _o2, stderr_feat) = run_hook_with_input(input, Some(&root_feat));
    assert_eq!(
        code_feat, 0,
        "core must consume cwd: git commit with cwd on feature branch allows; stderr={stderr_feat}"
    );
}

#[test]
fn validate_pretool_reads_payload_cwd_engages_gate() {
    // validate_pretool must resolve its cwd from the payload `cwd`
    // field, not env::current_dir(). The single resolved cwd feeds all
    // five cwd consumers documented on run_impl_main (branch detection,
    // main_root, flow_active, the agent-prompt worktree_root, and the
    // Layer 10/11 + halt gates); they read one `cwd` binding so a
    // payload cwd reaching any one reaches all. Layer 10 is the
    // observable witness here: the payload cwd points at a git repo on
    // the integration branch while the process's real cwd is a non-git
    // tempdir. With the payload honored, the git-commit invocation
    // engages Layer 10 (exit 2). Reading env::current_dir() (the
    // non-git real cwd) would resolve no branch and allow (exit 0).
    let (_dir_repo, repo) = setup_repo_on_branch("main");
    let other = tempfile::tempdir().expect("real cwd tempdir");
    let real_cwd = other.path().canonicalize().expect("canonicalize");
    let input = format!(
        r#"{{"cwd":"{}","tool_input":{{"command":"git commit -m \"x\""}}}}"#,
        repo.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input(&input, Some(&real_cwd));
    assert_eq!(
        code, 2,
        "payload cwd on the integration branch must engage Layer 10; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn t1_bare_git_commit_on_main_blocks() {
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "git commit on main must block; stderr={stderr}");
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name the branch 'main'; got: {stderr}"
    );
}

#[test]
fn t5_git_commit_dash_f_on_main_blocks() {
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git commit -F /tmp/msg"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "git commit -F on main must block; stderr={stderr}");
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name the branch 'main'; got: {stderr}"
    );
}

#[test]
fn t6_git_commit_amend_on_main_blocks() {
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git commit --amend"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "git commit --amend on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name the branch 'main'; got: {stderr}"
    );
}

#[test]
fn t2_git_commit_on_feature_branch_in_worktree_allows() {
    // Fixture branch `feat-x` differs from default_branch_in's "main"
    // fallback (no remote configured). Layer 10 does not fire.
    let (_dir, root) = setup_repo_on_branch("feat-x");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git commit on feature branch must allow; stderr={stderr}"
    );
}

#[test]
fn t3_git_commit_on_feature_branch_in_main_repo_allows() {
    // The hook does not distinguish a worktree from a main repo —
    // only the resolved branch matters.
    let (_dir, root) = setup_repo_on_branch("feat-x");
    let input = r#"{"tool_input": {"command": "git commit -m \"y\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git commit on feature branch must allow; stderr={stderr}"
    );
}

#[test]
fn t4_git_commit_on_staging_default_repo_blocks() {
    // Configure `origin/HEAD` to `origin/staging` so default_branch_in
    // returns "staging" rather than the hardcoded fallback. The block
    // message names the staging branch — proving Layer 10 honours the
    // actual integration branch.
    let (_dir, root) = setup_repo_on_branch("staging");
    let _ = Command::new("git")
        .args(["remote", "add", "origin", root.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("git remote add");
    let _ = Command::new("git")
        .args(["update-ref", "refs/remotes/origin/staging", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("git update-ref");
    let _ = Command::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/staging",
        ])
        .current_dir(&root)
        .output()
        .expect("git symbolic-ref");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "git commit on staging must block; stderr={stderr}");
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("staging"),
        "stderr should name the branch 'staging' (not 'main'); got: {stderr}"
    );
}

#[test]
fn t14_git_status_on_main_allows() {
    // Layer 10 only fires on `git ... commit`. `git status` is a
    // different subcommand → is_commit_invocation returns false →
    // the hook does not check the branch.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git status"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 0, "git status on main must allow; stderr={stderr}");
}

#[test]
fn t17_git_commit_detached_head_allows() {
    // Detached HEAD: `git branch --show-current` returns empty,
    // current_branch_in reports None, the `?` in
    // check_commit_on_integration short-circuits → no block.
    let (_dir, root) = setup_repo_on_branch("main");
    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("git rev-parse");
    let sha = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    let _ = Command::new("git")
        .args(["update-ref", "--no-deref", "HEAD", &sha])
        .current_dir(&root)
        .output()
        .expect("detach HEAD");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git commit on detached HEAD must allow; stderr={stderr}"
    );
}

#[test]
fn t18_git_commit_in_non_git_tempdir_allows() {
    // Cwd is not a git repo. current_branch_in reports None → no
    // block. The hook never blocks when it cannot resolve a branch
    // because that scenario also can never produce a real commit on
    // the integration branch.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git commit in non-git dir must allow; stderr={stderr}"
    );
}

#[test]
fn t19_git_ci_alias_on_main_allows_in_v1() {
    // Documented v1 gap: `git ci -m x` (alias) shows `ci` as the
    // second token, not `commit`. is_commit_invocation returns false
    // → allow. This test pins the boundary so a future widening of
    // the matcher is a deliberate decision.
    let (_dir, root) = setup_repo_on_branch("main");
    let _ = Command::new("git")
        .args(["config", "alias.ci", "commit"])
        .current_dir(&root)
        .output()
        .expect("git config alias");
    let input = r#"{"tool_input": {"command": "git ci -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git ci alias on main allows in v1; stderr={stderr}"
    );
}

#[test]
fn t20_xargs_git_commit_on_main_blocks_via_escape_hatch_layer() {
    // The `xargs git commit` shape is blocked structurally by Layer
    // 8 (`.claude/rules/no-escape-hatches.md` "Canonical
    // Escape-Hatch Shapes"). Layer 10's commit-invocation matcher
    // never sees the wrapped `git commit` because Layer 8 fires
    // first on the `xargs` first-token basename — the wrapper itself
    // is the escape hatch regardless of what is being wrapped.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "xargs git commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "xargs is blocked at Layer 8; stderr={stderr}");
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("xargs"));
    assert!(stderr.contains("escape hatch"));
}

#[test]
fn t9_bin_flow_finalize_commit_on_main_blocks() {
    // The other commit pathway: `bin/flow finalize-commit` runs the
    // commit machinery from inside FLOW's binary. On the integration
    // branch the hook must block it the same way it blocks
    // `git commit`.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "bin/flow finalize-commit on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name the branch 'main'; got: {stderr}"
    );
}

#[test]
fn t10_absolute_path_bin_flow_finalize_commit_on_main_blocks() {
    // The first token can be an absolute path to bin/flow when a
    // skill invokes the launcher via ${CLAUDE_PLUGIN_ROOT}/bin/flow.
    // The matcher must recognize the suffix `*/bin/flow` so absolute
    // paths block the same way as bare `bin/flow`.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "/Users/ben/code/flow/bin/flow finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "absolute /Users/.../bin/flow finalize-commit on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
}

#[test]
fn t11_bin_flow_finalize_commit_in_worktree_allows() {
    // From a feature-branch fixture (representing a worktree),
    // bin/flow finalize-commit allows because current_branch
    // (feat-x) differs from default_branch_in's "main" fallback.
    let (_dir, root) = setup_repo_on_branch("feat-x");
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "bin/flow finalize-commit on feature branch must allow; stderr={stderr}"
    );
}

#[test]
fn t12_bin_flow_start_gate_on_main_allows() {
    // start-gate is a sibling bin/flow subcommand that does NOT
    // perform a commit through Claude's Bash tool path. Layer 10
    // must not match it. This pins the boundary so the matcher
    // doesn't over-fire on every bin/flow invocation.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow start-gate --branch feat-x"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "bin/flow start-gate on main must allow; stderr={stderr}"
    );
}

#[test]
fn t13_bin_flow_start_workspace_on_main_allows() {
    // Sibling case: start-workspace also runs from the start lock on
    // main and must not be blocked by Layer 10.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow start-workspace feat-x --branch feat-x"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "bin/flow start-workspace on main must allow; stderr={stderr}"
    );
}

#[test]
fn t21_unknown_launcher_finalize_commit_allows() {
    // Boundary: an unrelated launcher with `finalize-commit` as the
    // second token must NOT match. is_bin_flow_token rejects the
    // first token (neither bare `bin/flow` nor a `*/bin/flow` suffix)
    // → arm returns false → allow. Pins the matcher's launcher
    // surface so it cannot widen accidentally.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "node finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "unknown-launcher finalize-commit must allow; stderr={stderr}"
    );
}

#[test]
fn t7_git_dash_c_key_value_commit_on_main_blocks() {
    // `git -c user.email=x commit -m x` slips a config override
    // between `git` and the subcommand. The matcher must skip past
    // `-c <value>` and find `commit` as the effective subcommand.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git -c user.email=x commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "git -c k=v commit on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name 'main'; got: {stderr}"
    );
}

#[test]
fn t8_git_dash_c_to_main_from_worktree_blocks() {
    // Adversarial: hook cwd is a feature-branch worktree, but the
    // command uses `git -C <main_repo_path>` to redirect git's
    // effective cwd onto the integration branch. Layer 10 must
    // resolve the branch from BOTH the hook cwd AND the `-C` path
    // and block when EITHER matches the integration branch.
    let (_main_dir, main_root) = setup_repo_on_branch("main");
    let (_feat_dir, feat_root) = setup_repo_on_branch("feat-x");
    let main_path = main_root.to_str().expect("utf-8 main path");
    let cmd = format!(
        r#"{{"tool_input": {{"command": "git -C {} commit -m \"x\""}}}}"#,
        main_path
    );
    let (code, _stdout, stderr) = run_hook_with_input(&cmd, Some(&feat_root));
    assert_eq!(
        code, 2,
        "git -C <main_path> commit from feat-x must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name 'main' (the -C target's branch); got: {stderr}"
    );
}

#[test]
fn t15_quoted_git_commit_on_main_blocks() {
    // `'git' commit -m x` quotes the command name. Bash dequotes it
    // before exec, so the matcher must dequote the first token before
    // comparing it to "git".
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "'git' commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "'git' commit on main must block (dequoted); stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name 'main'; got: {stderr}"
    );
}

#[test]
fn t16_bash_dash_c_git_commit_on_main_blocks() {
    // `bash -c '<inner>'` is a shell-eval escape hatch regardless
    // of the inner content. Layer 8
    // (`.claude/rules/no-escape-hatches.md`) fires before Layer 10's
    // integration-branch matcher unwraps the `-c` argument, so the
    // block message is the escape-hatch citation rather than the
    // integration-branch citation. The intent of the original test
    // — `bash -c 'git commit ...'` is rejected on main — is preserved
    // by the earlier and stronger Layer 8 block.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bash -c 'git commit -m \"x\"'"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "bash -c 'git commit ...' on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("escape hatch") || stderr.contains("main"),
        "stderr should cite the escape-hatch class or name the integration branch; got: {stderr}"
    );
}

#[test]
fn t23_sh_dash_c_git_commit_on_main_blocks() {
    // Sibling of T16 — `sh` and `bash` are both POSIX-compatible
    // shells that take `-c <script>`. The matcher must handle both.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "sh -c 'git commit -m \"x\"'"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "sh -c 'git commit ...' on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
}

#[test]
fn t24_git_dash_c_with_no_value_allows() {
    // Boundary: `git -c` with no value (or no subcommand after the
    // value) — the matcher consumes `-c` plus the next token (None
    // here), the loop exhausts without finding a subcommand, and
    // returns Some(_) == "commit" → false. Layer 10 doesn't fire.
    // Pins the "next_git_subcommand returns None on exhaustion"
    // branch so a refactor that loses the loop-end fallback fails CI.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git -c"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "bare 'git -c' with no value must allow; stderr={stderr}"
    );
}

#[test]
fn t25_git_dash_uppercase_c_with_no_path_allows() {
    // Boundary: `git -C` with no path — extract_dash_c_path's
    // `tokens.next()` after `-C` returns None, so the function
    // returns None and check_commit_on_integration only checks the
    // hook cwd (which is `main`). is_commit_invocation also returns
    // false because next_git_subcommand exhausts without finding a
    // subcommand → Layer 10 does not fire → allow.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git -C"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "bare 'git -C' with no path must allow; stderr={stderr}"
    );
}

#[test]
fn t26_bin_flow_with_flag_before_finalize_commit_blocks() {
    // The `bin/flow` arm of `is_commit_invocation_inner` matches
    // `finalize-commit` as ANY subsequent token (not just the
    // immediate next one). bin/flow today has no global flags, but
    // a future addition like `--verbose` or `--log-level <value>`
    // must not bypass the gate. Pin the defensive matcher so the
    // bypass cannot regress.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow --verbose finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "bin/flow --verbose finalize-commit on main must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("main"),
        "stderr should name the branch 'main'; got: {stderr}"
    );
}

#[test]
fn t27_git_dash_c_to_nonexistent_path_from_feature_branch_allows() {
    // Boundary: hook cwd is a feature branch, command uses
    // `git -C /nonexistent commit`. match_branch_at(cwd) returns
    // None (current=feat-x ≠ integration=main, the "current !=
    // integration" branch); extract_dash_c_path returns Some, but
    // match_branch_at(non-git path) also returns None (no current
    // branch). check_commit_on_integration falls through to
    // None → allow. Pins the path-pair "both candidates miss"
    // branch in the dispatcher.
    let (_dir, root) = setup_repo_on_branch("feat-x");
    let input = r#"{"tool_input": {"command": "git -C /nonexistent/path commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "feat-x cwd + non-git -C path must allow; stderr={stderr}"
    );
}

// --- layer_10_active_flow ---
//
// Layer 10 also fires when the hook's effective cwd resolves to a
// feature-branch worktree that has an active FLOW state file at
// `.flow-states/<branch>/state.json` — the second trigger context
// the gate covers. The fixture `setup_active_flow_worktree` builds
// the minimal layout the production helpers need:
//   <root>/.claude/settings.json          → find_settings_and_root_from
//   <root>/.flow-states/<branch>/state.json → is_flow_active (when present)
//   <root>/.worktrees/<branch>/.git       → detect_branch_from_path
// Tests in this section spawn the hook with cwd at
// `<root>/.worktrees/<branch>/` (or the unrelated-cwd variant for the
// `-C` interaction case) and assert the active-flow message contains
// both "active flow" and "/flow:flow-commit".

/// Build a fixture that satisfies `match_active_flow_at` for the named
/// branch. Returns `(TempDir, project_root, worktree_path)` — pass
/// `worktree_path` as the hook cwd. When `with_state_file` is false,
/// the state file is omitted so `is_flow_active` returns false (used
/// for the negative-context tests).
fn setup_active_flow_worktree(
    branch: &str,
    with_state_file: bool,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");

    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    // Initialize a real git repo at root with main + origin/HEAD so
    // `git::default_branch_in` resolves cleanly.
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output();
    };
    run_git(&["init", "-b", "main"], &root);
    run_git(&["config", "user.email", "t@t.com"], &root);
    run_git(&["config", "user.name", "T"], &root);
    run_git(&["config", "commit.gpgsign", "false"], &root);
    run_git(&["commit", "--allow-empty", "-m", "init"], &root);
    run_git(&["update-ref", "refs/remotes/origin/main", "HEAD"], &root);
    run_git(
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
        &root,
    );

    if with_state_file {
        let states_dir = root.join(".flow-states").join(branch);
        std::fs::create_dir_all(&states_dir).unwrap();
        std::fs::write(states_dir.join("state.json"), "{}").unwrap();
    }

    let worktree = root.join(".worktrees").join(branch);
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: ../../.git/worktrees/{branch}"),
    )
    .unwrap();

    (dir, root, worktree)
}

#[test]
fn layer_10_blocks_bare_git_commit_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "git commit during active flow must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("active flow"),
        "stderr should name 'active flow' context; got: {stderr}"
    );
    assert!(
        stderr.contains("/flow:flow-commit"),
        "stderr should redirect to /flow:flow-commit; got: {stderr}"
    );
}

#[test]
fn layer_10_blocks_quoted_git_commit_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "'git' commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "'git' commit during active flow must block (dequoted); stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_blocks_git_dash_c_kv_commit_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "git -c user.email=x commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "git -c k=v commit during active flow must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_blocks_bash_dash_c_git_commit_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "bash -c 'git commit -m \"x\"'"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "bash -c 'git commit ...' during active flow must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    // Layer 8's structural escape-hatch block fires before Layer 10's
    // commit-during-flow gate because `bash -c` is itself a shell-eval
    // escape hatch regardless of what's wrapped inside it. The block
    // still fires; the message is the no-escape-hatches.md citation
    // rather than the active-flow citation. The test's intent —
    // `bash -c 'git commit ...'` is rejected during an active flow —
    // is preserved by the earlier and stronger Layer 8 block.
    assert!(stderr.contains("escape hatch") || stderr.contains("active flow"));
}

#[test]
fn layer_10_blocks_bin_flow_finalize_commit_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "bin/flow finalize-commit during active flow must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
    assert!(stderr.contains("/flow:flow-commit"));
}

#[test]
fn layer_10_blocks_bin_flow_flag_finalize_commit_on_active_flow_worktree() {
    // The `bin/flow` arm matches `finalize-commit` as ANY subsequent
    // token. A future global flag like `--verbose` must not bypass the
    // active-flow gate either.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "bin/flow --verbose finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "bin/flow <flag> finalize-commit during active flow must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_blocks_git_dash_c_path_to_active_flow_worktree() {
    // Hook cwd is unrelated (no git, no .claude/, no .flow-states/),
    // but the command uses `git -C <active-flow-worktree-path> commit`.
    // The -C target's branch resolves via detect_branch_from_path's
    // `.worktrees/<branch>/` marker; find_settings_and_root_from on
    // the target walks up to the active-flow root; is_flow_active
    // returns true → active-flow fires for the -C target.
    let (_flow_dir, _flow_root, flow_cwd) = setup_active_flow_worktree("feat", true);
    let unrelated = tempfile::tempdir().expect("tempdir");
    let unrelated_root = unrelated.path().canonicalize().expect("canonicalize");
    let target = flow_cwd.to_str().expect("utf-8 path");
    let cmd = format!(
        r#"{{"tool_input": {{"command": "git -C {} commit -m \"x\""}}}}"#,
        target
    );
    let (code, _stdout, stderr) = run_hook_with_input(&cmd, Some(&unrelated_root));
    assert_eq!(
        code, 2,
        "git -C <active-flow-worktree> commit from unrelated cwd must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(
        stderr.contains("active flow"),
        "stderr should name 'active flow' context (the -C target's predicate); got: {stderr}"
    );
}

#[test]
fn layer_10_passes_git_status_on_active_flow_worktree() {
    // Read-only git is not a commit invocation → Layer 10 is silent.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "git status"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "git status during active flow must allow; stderr={stderr}"
    );
}

#[test]
fn layer_10_passes_git_diff_cached_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "git diff --cached"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "git diff --cached during active flow must allow; stderr={stderr}"
    );
}

#[test]
fn layer_10_passes_git_log_on_active_flow_worktree() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", true);
    let input = r#"{"tool_input": {"command": "git log --oneline -5"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "git log during active flow must allow; stderr={stderr}"
    );
}

#[test]
fn layer_10_passes_git_commit_on_feature_branch_without_state_file() {
    // Pre-flow editing scenario: settings.json present (so the FLOW
    // project is discoverable) but no state file at
    // .flow-states/<branch>/state.json. is_flow_active returns false
    // → active-flow predicate returns None → Layer 10 silent.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "git commit on feature worktree without state file must allow; stderr={stderr}"
    );
}

/// Drives the `cwd.is_none()` branch in `validate_pretool::run()` —
/// `env::current_dir()` returns `Err` when the cwd inode has been
/// unlinked. The hook must fall through Layer 10 cleanly (no panic,
/// no Layer 10 fire) and exit 0 on the allowed `git status` payload.
///
/// Mirrors the production-binding test for the same branch in
/// `tests/adversarial_agent_block.rs::validate_pretool_with_stale_cwd_does_not_panic`,
/// brought into the mirrored test binary so the per-file gate against
/// `src/hooks/validate_pretool.rs` exercises the line.
#[cfg(unix)]
#[test]
fn layer_10_stale_cwd_does_not_panic_or_block() {
    use std::os::unix::process::CommandExt;

    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");
    let cwd = root.join("doomed");
    std::fs::create_dir(&cwd).expect("mkdir doomed");

    let preexec_path =
        std::ffi::CString::new(cwd.to_str().expect("utf8").as_bytes()).expect("CString");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_flow-rs"));
    cmd.args(["hook", "validate-pretool"])
        .env_remove("FLOW_CI_RUNNING")
        .env_remove("FLOW_SIMULATE_BRANCH")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: libc::rmdir is POSIX async-signal-safe. The closure
    // allocates nothing, produces no panic surface, and does not
    // interact with any parent-process state.
    unsafe {
        cmd.pre_exec(move || {
            libc::rmdir(preexec_path.as_ptr());
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn flow-rs");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"tool_input":{"command":"git status"}}"#)
        .unwrap();
    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked at"),
        "validate-pretool must not panic with stale cwd; stderr={stderr}"
    );
    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "stale cwd + allowed command must exit 0; stderr={stderr}"
    );
}

#[test]
fn layer_10_passes_git_commit_in_unrelated_git_repo() {
    // Cwd is an unrelated git repo: no .claude/settings.json walking
    // up from cwd → find_settings_and_root_from returns (None, None)
    // → match_active_flow_at returns None. Branch resolves to
    // "feat-x" via the real git subprocess (the existing fixture),
    // so match_branch_at returns None ("feat-x" != "main"). Layer 10
    // silent → allow.
    let (_dir, root) = setup_repo_on_branch("feat-x");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "git commit in unrelated git repo must allow; stderr={stderr}"
    );
}

// --- layer_10_skill_commit_carveout ---
//
// The legitimate skill-driven commit path is `/flow:flow-commit` →
// `bin/flow finalize-commit`. The flow-code, flow-review, and
// flow-learn skills all set `_continue_pending=commit` on the state
// file immediately before invoking /flow:flow-commit, so the field is
// the marker Layer 10 checks. When the carve-out fires, the hook
// allows `bin/flow ... finalize-commit` (and only that shape) through
// the active-flow gate. `git commit` is never carved out — the skill
// never invokes raw git commit, so the marker plus a `git commit`
// command always indicates a bypass attempt.

/// Like `setup_active_flow_worktree(branch, true)` but lets the test
/// specify the state.json content. Use this to write a state file
/// with `_continue_pending=commit` (the carve-out marker) or any
/// other shape needed to drive `state_continue_pending_is_commit`.
fn setup_active_flow_worktree_with_state(
    branch: &str,
    state_json: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");

    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    // Initialize a real git repo at root with main + origin/HEAD so
    // `git::default_branch_in` resolves cleanly.
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        let _ = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output();
    };
    run_git(&["init", "-b", "main"], &root);
    run_git(&["config", "user.email", "t@t.com"], &root);
    run_git(&["config", "user.name", "T"], &root);
    run_git(&["config", "commit.gpgsign", "false"], &root);
    run_git(&["commit", "--allow-empty", "-m", "init"], &root);
    run_git(&["update-ref", "refs/remotes/origin/main", "HEAD"], &root);
    run_git(
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
        &root,
    );

    let states_dir = root.join(".flow-states").join(branch);
    std::fs::create_dir_all(&states_dir).unwrap();
    std::fs::write(states_dir.join("state.json"), state_json).unwrap();

    let worktree = root.join(".worktrees").join(branch);
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: ../../.git/worktrees/{branch}"),
    )
    .unwrap();

    (dir, root, worktree)
}

#[test]
fn layer_10_carveout_allows_bin_flow_finalize_commit_when_continue_pending_is_commit() {
    // Skill choreography: /flow:flow-commit fires (most recent
    // assistant Skill in the transcript) AND flow-code (or sibling)
    // wrote _continue_pending=commit, AND the command shape is
    // bin/flow finalize-commit. All three carve-out conditions hold,
    // so Layer 10 passes through. CI runs inside finalize-commit and
    // the commit lands.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "skill-invoked finalize-commit must pass; stderr={stderr}"
    );
}

#[test]
fn layer_10_carveout_allows_absolute_bin_flow_finalize_commit_when_marker_set() {
    // Skill bash blocks invoke `${CLAUDE_PLUGIN_ROOT}/bin/flow
    // finalize-commit ...` which expands to an absolute-path form.
    // The carve-out's command-shape predicate uses `is_bin_flow_token`
    // which accepts both bare and `*/bin/flow` suffix forms. All
    // three carve-out conditions are exercised here so the
    // absolute-path shape also passes.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "/Users/me/code/flow/bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "absolute-path skill-invoked finalize-commit must pass; stderr={stderr}"
    );
}

#[test]
fn layer_10_carveout_does_not_apply_to_git_commit_even_with_marker() {
    // Marker is present but command shape is `git commit`. The skill
    // carve-out is finalize-commit-only by design — raw git commit
    // is never legitimate during a flow regardless of state. Block.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "git commit during active flow must block even with marker; stderr={stderr}"
    );
    assert!(
        stderr.contains("BLOCKED"),
        "stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("active flow"),
        "stderr should name 'active flow' context; got: {stderr}"
    );
}

#[test]
fn layer_10_carveout_blocks_finalize_commit_when_continue_pending_absent() {
    // Active state file but no _continue_pending key. The carve-out
    // requires the marker to be definitively the string "commit";
    // absence is fail-closed. Block.
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", r#"{}"#);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "finalize-commit without _continue_pending marker must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("active flow"),
        "stderr should name 'active flow' context; got: {stderr}"
    );
}

#[test]
fn layer_10_carveout_blocks_finalize_commit_when_continue_pending_is_other_value() {
    // Marker is set but to a value other than "commit" (e.g. an old
    // value left by a prior skill round, or a hand-edited state).
    // The carve-out requires exact equality with "commit". Block.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "review"}"#);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "finalize-commit with non-commit marker must block; stderr={stderr}"
    );
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_carveout_blocks_finalize_commit_when_continue_pending_wrong_type() {
    // Marker present but as a non-string (e.g. number or null).
    // `as_str()` returns None → fail-closed → block. Tolerates
    // legacy or corrupted state without bypassing the gate.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": 1}"#);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "finalize-commit with non-string marker must block; stderr={stderr}"
    );
}

#[test]
fn layer_10_carveout_blocks_finalize_commit_when_state_file_is_malformed_json() {
    // `is_flow_active` reports active (state.json exists with
    // `.is_file() == true`), so the active-flow predicate fires and
    // the carve-out is consulted. `state_continue_pending_is_commit`
    // reads the file then calls `serde_json::from_str` which returns
    // Err on malformed content. Fail-closed → carve-out doesn't
    // apply → block. Drives the parse-error let-else arm.
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", "this is not json");
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "finalize-commit with malformed state.json must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("active flow"),
        "stderr should name 'active flow' context; got: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn layer_10_carveout_blocks_finalize_commit_when_state_file_is_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    // `is_flow_active`'s `.is_file()` succeeds even when the file's
    // read perms are 000 — metadata is fetched from the parent dir,
    // not by reading content. The downstream
    // `state_continue_pending_is_commit` then attempts
    // `read_to_string`, which returns `Err(EACCES)`. Fail-closed →
    // carve-out doesn't apply → block. This test exercises the
    // `Err` arm of the read so 100/100/100 covers the let-else.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let state_path = root.join(".flow-states").join("feat").join("state.json");

    let mut perms = std::fs::metadata(&state_path).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&state_path, perms).unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));

    // Restore perms before any assertion can short-circuit tempdir
    // cleanup.
    let mut perms = std::fs::metadata(&state_path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&state_path, perms).unwrap();

    assert_eq!(
        code, 2,
        "finalize-commit with unreadable state.json must block; stderr={stderr}"
    );
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_carveout_allows_bash_c_wrapped_finalize_commit() {
    // Layer 8 (`.claude/rules/no-escape-hatches.md`) blocks
    // `bash -c '...'` as a shell-eval escape hatch regardless of the
    // wrapped inner command. The active-flow carve-out at Layer 10
    // recognizes a `bash -c`-wrapped finalize-commit shape only for
    // callers that bypass Layer 8 — and during an active flow no
    // such caller exists, because /flow:flow-commit invokes
    // `bin/flow finalize-commit` directly via the Bash tool (no
    // bash -c wrapper). The skill-commit carve-out is therefore
    // unreachable from the active-flow path when the wrapper is
    // bash -c. The legitimate skill commit path bypasses Layer 8
    // because it does not pass through bash -c at all.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let input = r#"{"tool_input": {"command": "bash -c 'bin/flow finalize-commit feat'"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "bash -c wrapper is itself a shell-eval escape hatch and must block at Layer 8; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("escape hatch"));
    assert!(stderr.contains("no-escape-hatches.md"));
}

#[test]
fn layer_10_carveout_does_not_apply_on_integration_branch() {
    // Even with the marker set, a finalize-commit invocation whose
    // resolved branch IS the integration branch must block — the
    // carve-out is for active-flow context, not integration-branch
    // context. `match_branch_at` fires before `check_active_flow_at`
    // in `check_commit_during_flow`, so the integration-branch
    // message wins.
    let (_dir, root) = setup_repo_on_branch("main");
    let states_dir = root.join(".flow-states").join("main");
    std::fs::create_dir_all(&states_dir).unwrap();
    std::fs::write(
        states_dir.join("state.json"),
        r#"{"_continue_pending": "commit"}"#,
    )
    .unwrap();
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "finalize-commit on integration branch must block even with marker; stderr={stderr}"
    );
    assert!(
        stderr.contains("integration branch"),
        "stderr should name integration-branch context; got: {stderr}"
    );
}

// --- skill_commit_closure: transcript-walker carve-out condition ---
//
// The third AND-combined condition on the skill-commit carve-out
// (per `.claude/rules/no-escape-hatches.md` Layer C). Each test
// fixtures the state.json marker, builds a transcript JSONL that
// exercises the walker's most_recent_skill_since_user semantics,
// and asserts the gate behavior.

#[test]
fn layer_10_closure_blocks_when_transcript_shows_different_skill() {
    // Marker present AND finalize-commit shape, but the most recent
    // assistant Skill call is `decompose:decompose` (not
    // flow:flow-commit). The walker's third condition fails so the
    // carve-out does not apply.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = assistant_skill_jsonl("decompose:decompose");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "finalize-commit with non-flow-commit Skill in transcript must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
    assert!(stderr.contains("no-escape-hatches.md"));
}

#[test]
fn layer_10_closure_blocks_when_no_skill_since_user_turn() {
    // The user turn appears AFTER the Skill call, so the walker
    // returns None (no Skill call since the most recent user turn).
    // The third condition fails → block.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-commit"),
        user_jsonl("follow-up prompt")
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "user turn after the Skill call invalidates the carve-out; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_closure_blocks_when_marker_absent_but_transcript_shows_flow_commit() {
    // Belt-and-suspenders: even though the walker would return
    // Some("flow:flow-commit"), the second condition
    // (_continue_pending == "commit") fails because the state has
    // a different value. The marker is preserved as a precondition.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "review"}"#);
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "missing marker must block even when transcript shows flow:flow-commit; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_closure_blocks_when_transcript_path_missing() {
    // hook input omits transcript_path entirely. The walker check
    // sees None and returns false, so the third condition fails
    // even though the marker is set. Fail-closed: missing transcript
    // means the surrounding skill choreography cannot be verified.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "missing transcript_path must block even with marker set; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_closure_blocks_when_transcript_path_invalid() {
    // transcript_path supplied but rooted outside <home>/.claude/projects/.
    // is_safe_transcript_path rejects, the walker returns None, the
    // third condition fails. Block.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    // Write a JSONL file outside the safe prefix.
    let bad_path = root.join("not-in-projects.jsonl");
    std::fs::write(&bad_path, assistant_skill_jsonl("flow:flow-commit")).unwrap();
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        bad_path.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "transcript_path outside ~/.claude/projects/ must be rejected; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn layer_10_closure_integration_branch_message_wins_over_active_flow() {
    // When the resolved branch IS the integration branch AND a state
    // file exists, match_branch_at fires first inside
    // check_commit_during_flow, so the integration-branch message
    // wins over the active-flow message. Even with all three
    // carve-out conditions met, the integration-branch context is
    // never carved out.
    let (_dir, root) = setup_repo_on_branch("main");
    let states_dir = root.join(".flow-states").join("main");
    std::fs::create_dir_all(&states_dir).unwrap();
    std::fs::write(
        states_dir.join("state.json"),
        r#"{"_continue_pending": "commit"}"#,
    )
    .unwrap();
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "integration-branch must block even when all carve-out conditions hold; stderr={stderr}"
    );
    assert!(
        stderr.contains("integration branch"),
        "stderr should name the integration-branch context; got: {stderr}"
    );
}

#[test]
fn layer_10_closure_block_message_cites_no_escape_hatches_rule() {
    // The active-flow block message must cite
    // .claude/rules/no-escape-hatches.md (the citation contract test
    // in Task 11 enforces this forward). Exercise the block path
    // and check the message content.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    // No transcript fixture → carve-out fails → block.
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(code, 2);
    assert!(stderr.contains("/flow:flow-commit"));
    assert!(stderr.contains("no-escape-hatches.md"));
}

// --- layer_10_bootstrap_carveout ---
//
// The bootstrap-skill carve-out on Layer 10's integration-branch
// context. The flow-start Step 2 (deps repair commit) and flow-prime
// Step 6 (setup writes commit) skills invoke `/flow:flow-commit`
// while cwd is on the integration branch. Without a carve-out, the
// integration-branch context blocks every such commit. The
// integration-branch context has no per-branch state file, so the
// carve-out cannot mirror the active-flow carve-out's
// `_continue_pending=commit` marker. Instead it uses two
// AND-combined walker conditions:
//
//   1. `is_finalize_commit_invocation(command)` (the command shape)
//   2. `most_recent_skill_since_user(path, home)` returns
//      `Some("flow:flow-commit")` — the most recent assistant Skill
//      since the most recent user turn is `flow:flow-commit`
//   3. `any_skill_in_set_since_user(path, home, BOOTSTRAP_SKILLS)`
//      returns true — a sanctioned bootstrap parent
//      (`flow:flow-start` or `flow:flow-prime`) appears in the
//      assistant Skill chain since the most recent user turn
//
// Raw `git commit` is never carved out; the carve-out's shape
// predicate matches `bin/flow finalize-commit` only.

#[test]
fn layer_10_bootstrap_carveout_allows_on_main_when_flow_start_chain() {
    // Bootstrap window: cwd is the integration branch (`main`), the
    // transcript shows Skill(flow:flow-start) followed by
    // Skill(flow:flow-commit) since the most recent user turn, and
    // the command shape is `bin/flow finalize-commit`. All three
    // bootstrap-carveout conditions hold → Layer 10 passes through.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap-skill carve-out must pass on main with flow-start chain; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_allows_for_flow_prime() {
    // Second sanctioned-parent entry: Skill(flow:flow-prime) plus
    // Skill(flow:flow-commit) on the integration branch. The
    // bootstrap carve-out fires for either sanctioned parent.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-prime"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap-skill carve-out must pass on main with flow-prime chain; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_allows_for_flow_release() {
    // Third sanctioned-parent entry: Skill(flow-release) is both the
    // initiating skill AND the most-recent skill, because flow-release
    // calls `bin/flow finalize-commit` directly rather than delegating
    // to `/flow:flow-commit`. The transcript chain is a single
    // Skill(flow-release) — no separate flow-commit invocation. The
    // bare name (no `flow:` prefix) reflects the literal `input.skill`
    // value Claude Code emits for the project-local maintainer skill
    // at `.claude/skills/flow-release/`.
    //
    // The most-recent-skill predicate is two-arm: it accepts either
    // `flow:flow-commit` (delegated commit path used by flow-start
    // and flow-prime, both plugin-marketplace skills at
    // `skills/<name>/`) or `flow-release` (direct commit path; the
    // skill is project-local). The bootstrap-parent walker scans
    // `BOOTSTRAP_SKILLS` for `flow-release` and finds it as a
    // sanctioned parent of itself.
    //
    // Per-skill trust contract: when most-recent is flow-commit,
    // the trust is the standard diff-review choreography. When
    // most-recent is flow-release, the trust is the release skill's
    // own internal review window: Step 3 displays
    // `git log <last_tag>..HEAD`, Step 4 drafts release notes
    // against that list, and Step 7 writes an explicit
    // "Release v<new_version>" commit message file before
    // finalize-commit reads it.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("flow-release");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap-skill carve-out must pass on main with flow-release chain; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_normalizes_flow_release_uppercase() {
    // `transcript_shows_commit_window_skill` must normalize the skill
    // string from `most_recent_skill_since_user` before the byte
    // comparison so case- and whitespace-variant emissions cannot
    // drift past the gate. Sibling
    // `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` already
    // normalizes via `normalize_gate_input`; this test pins the
    // symmetry property required by
    // `.claude/rules/security-gates.md` "Normalize Before Comparing".
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("FLOW-RELEASE");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "uppercase FLOW-RELEASE must normalize and pass the carve-out; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_normalizes_flow_release_trailing_whitespace() {
    // Trailing whitespace on the emitted skill string must be
    // tolerated by the normalize-before-comparing discipline. This
    // test pairs with the uppercase variant above to lock in the
    // symmetry with `any_skill_in_set_since_user`.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("flow-release  ");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "trailing-whitespace flow-release must normalize and pass; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_allows_finalize_commit_on_staging_during_flow_start() {
    // Configure `origin/HEAD` to `origin/staging` so
    // `default_branch_in` returns "staging". The carve-out names no
    // branch — it gates on `is_finalize_commit_invocation` +
    // `most_recent_skill_since_user == Some("flow:flow-commit")` +
    // `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` regardless of
    // which branch the integration trunk is. Mirrors
    // `t4_git_commit_on_staging_default_repo_blocks` to pin the
    // branch-agnostic property.
    let (_dir, root) = setup_repo_on_branch("staging");
    let _ = Command::new("git")
        .args(["remote", "add", "origin", root.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("git remote add");
    let _ = Command::new("git")
        .args(["update-ref", "refs/remotes/origin/staging", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("git update-ref");
    let _ = Command::new("git")
        .args([
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/staging",
        ])
        .current_dir(&root)
        .output()
        .expect("git symbolic-ref");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit staging"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap-skill carve-out must pass on staging integration branch; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_blocks_dash_c_target_with_git_commit() {
    // `is_finalize_commit_invocation` only matches
    // `bin/flow finalize-commit`, not `git -C ... commit`. When a
    // command targets the integration branch via `git -C` AND
    // the transcript shows the bootstrap chain, the carve-out's
    // command-shape predicate fails, so the block fires. Pins that
    // the carve-out is finalize-commit-only by design — there is
    // no git-prefixed escape hatch.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "git -C {} commit -m \"x\""}}, "transcript_path": "{}"}}"#,
        root.to_string_lossy(),
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "git -C <main> commit must block even with bootstrap chain; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_for_flow_orchestrate_parent() {
    // Sanctioned-parent set is closed: {flow:flow-start,
    // flow:flow-prime}. Skill(flow:flow-orchestrate) is NOT a
    // sanctioned parent. The walker's
    // `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` returns
    // false even though `most_recent_skill_since_user` would match
    // `flow:flow-commit`; the AND fails and the block fires.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-orchestrate"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "flow-orchestrate is not a sanctioned bootstrap parent — block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_raw_git_commit_even_with_chain() {
    // The bootstrap carve-out's first AND-combined condition is
    // `is_finalize_commit_invocation`, which matches
    // `bin/flow finalize-commit` only. A raw `git commit` invocation
    // — even with the sanctioned-parent chain AND a flow-commit Skill
    // in the transcript — fails the shape predicate, so the block
    // fires.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "git commit -m \"x\""}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "raw git commit must block even with bootstrap chain; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_after_user_turn_closes_window() {
    // After the user types another message, the walker stops at
    // that user turn going backward, so the older sanctioned parent
    // is invisible. Specifically: User → Skill(flow:flow-prime) →
    // Skill(flow:flow-commit) → User: "/flow:flow-commit" →
    // Skill(flow:flow-commit). The walker scans the second
    // bash invocation backward, hits the second flow-commit Skill,
    // hits the most recent real user turn, and stops without
    // finding the sanctioned parent → block fires.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}{}{}{}",
        user_jsonl("prime my project"),
        assistant_skill_jsonl("flow:flow-prime"),
        assistant_skill_jsonl("flow:flow-commit"),
        user_jsonl("now commit directly"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "user turn after sanctioned parent closes the carve-out window; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_when_transcript_path_missing() {
    // Hook input omits `transcript_path`.
    // `bootstrap_carveout_applies` early-returns false on the None
    // arm of `let Some(path) = transcript_path else`; the carve-out
    // cannot fire without a transcript. Block fires.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "missing transcript_path must block the bootstrap carve-out; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_when_transcript_path_invalid() {
    // `transcript_path` exists but is rooted outside
    // `<home>/.claude/projects/`. `is_safe_transcript_path` rejects
    // it, the walker returns false, and the AND fails → block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let bad_path = root.join("not-in-projects.jsonl");
    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    std::fs::write(&bad_path, jsonl).unwrap();
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        bad_path.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "transcript_path outside ~/.claude/projects/ must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn layer_10_bootstrap_carveout_blocks_user_direct_flow_commit_on_main() {
    // User types `/flow:flow-commit` directly on the integration
    // branch — the transcript shows ONLY Skill(flow:flow-commit) and
    // no sanctioned bootstrap parent.
    // `most_recent_skill_since_user` returns Some("flow:flow-commit"),
    // but `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` returns
    // false. The AND fails → block fires. Pins that the carve-out
    // is for skill-driven bootstrap windows, not arbitrary user-
    // initiated commits on the integration branch.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "user-direct /flow:flow-commit on main without bootstrap parent must block; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_does_not_apply_via_dash_c_target_for_cross_repo_safety() {
    // Cwd is a feature-branch tempdir (NOT the integration branch),
    // but the command is `git -C <main_root> commit` — a `-C` token
    // that shifts git's effective cwd onto the integration branch.
    // `extract_finalize_commit_branch_arg` returns None for a `git`
    // command, so the cwd path engages and the matcher checks BOTH
    // candidate cwds: the hook's process cwd and the `-C` target. The
    // first match_branch_at(cwd) returns None for the feature branch;
    // the -C target's match_branch_at fires. The bootstrap carve-out
    // is intentionally NOT applied at the -C target callsite
    // (cwd-only design) because the transcript walker is
    // session-scoped: a bootstrap chain in session activity for one
    // repo could otherwise authorize a commit redirected via -C to
    // another repo's integration branch. Legitimate bootstrap windows
    // always run with cwd ON the integration branch, so this
    // tightening has no production consumer cost.
    let (_main_dir, main_root) = setup_repo_on_branch("main");
    let (_feat_dir, feat_root) = setup_repo_on_branch("feat-x");
    let main_path = main_root.to_str().expect("utf-8 main path");

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&main_root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "git -C {} commit"}}, "transcript_path": "{}"}}"#,
        main_path,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) =
        run_hook_with_input_and_home(&input, Some(&feat_root), Some(&main_root));
    assert_eq!(
        code, 2,
        "bootstrap carve-out must NOT fire via -C target (cross-repo safety); stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_bootstrap_carveout_unaffected_by_active_flow_carveout() {
    // Feature-branch worktree (not integration branch) with
    // `_continue_pending=commit` AND a flow:flow-prime Skill in the
    // chain. Cwd is the feature-branch worktree, so `match_branch_at`
    // returns None and the integration-branch context (where the
    // bootstrap carve-out applies) is never consulted. The active-
    // flow carve-out's three conditions
    // (`is_finalize_commit_invocation` + marker == "commit" +
    // `transcript_shows_flow_commit`) gate the active-flow path
    // independently. The bootstrap carve-out's branch-agnostic
    // shape means a stray sanctioned-parent Skill in the chain
    // never weakens the active-flow path either.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-prime"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "feature-branch active-flow carve-out path is independent of bootstrap chain; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_allows_for_flow_release_user_typed_slash_command() {
    // flow-release is a user-only skill: Claude Code records the user
    // typing `/flow-release` as a user-role turn, never as an
    // assistant Skill tool_use. The transcript here is a single
    // user-typed `<command-name>/flow-release</command-name>` turn
    // with no assistant Skill. The bootstrap carve-out fires:
    // `bootstrap_carveout_applies`'s `commit_window` expression
    // recognizes the `/flow-release` user turn through its
    // `last_user_message_invokes_skill(path, "flow-release", home)`
    // OR-arm, and `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)`
    // recognizes the same user turn as the sanctioned bootstrap
    // parent. Layer 10 passes through.
    //
    // Regression guard: a future edit drops the
    // `last_user_message_invokes_skill` OR-arm from
    // `bootstrap_carveout_applies`'s `commit_window` expression, so a
    // maintainer typing `/flow-release` on the integration branch is
    // blocked from running the release version-bump commit.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = user_jsonl("<command-name>/flow-release</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "user-typed /flow-release must satisfy the bootstrap carve-out on the integration branch; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_allows_for_flow_prime_user_typed_slash_command() {
    // flow-prime is a user-only skill recorded as a user-role turn,
    // never an assistant Skill tool_use. The realistic flow-prime
    // bootstrap transcript is: user types `/flow:flow-prime`, then the
    // skill delegates the commit to `/flow:flow-commit` (an assistant
    // Skill). The bootstrap carve-out fires:
    // `transcript_shows_commit_window_skill` matches the
    // flow:flow-commit assistant Skill, and
    // `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` recognizes the
    // `/flow:flow-prime` user-typed turn as the sanctioned parent.
    //
    // Regression guard: a future edit drops the user-turn recognition
    // from `any_skill_in_set_since_user`, so the realistic flow-prime
    // bootstrap window (user-typed parent + delegated flow-commit) no
    // longer satisfies condition 3 of the carve-out and flow-prime
    // setup commits on the integration branch are blocked.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        user_jsonl("<command-name>/flow:flow-prime</command-name>"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "user-typed /flow:flow-prime parent + delegated flow-commit must satisfy the bootstrap carve-out; stderr={stderr}"
    );
}

#[test]
fn layer_10_bootstrap_carveout_blocks_when_no_bootstrap_skill_in_transcript() {
    // The transcript is a single plain user turn — no slash command,
    // no assistant Skill anywhere. `bootstrap_carveout_applies`'s
    // `commit_window` expression is a two-arm OR and both arms are
    // false: `transcript_shows_commit_window_skill` (assistant-Skill-
    // only, via `most_recent_skill_since_user`) finds no commit-window
    // skill, and `last_user_message_invokes_skill(path, "flow-release",
    // home)` finds no `/flow-release` user turn. With `commit_window`
    // false and `any_skill_in_set_since_user(BOOTSTRAP_SKILLS)` also
    // false, the carve-out cannot fire and Layer 10 blocks the commit
    // on the integration branch.
    //
    // Regression guard: a future edit makes the
    // `last_user_message_invokes_skill` OR-arm in
    // `bootstrap_carveout_applies` match any user turn rather than
    // only a `/flow-release` slash command, over-firing the carve-out
    // for an arbitrary integration-branch commit.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = user_jsonl("please commit the release for me");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "no bootstrap skill in transcript must block the integration-branch commit; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_active_flow_carveout_does_not_fire_on_flow_release_user_turn() {
    // The `/flow-release` user-turn recognition is scoped to the
    // integration-branch bootstrap carve-out
    // (`bootstrap_carveout_applies`), NOT the shared
    // `transcript_shows_commit_window_skill` predicate. The active-flow
    // carve-out (`check_active_flow_at`) consumes that shared predicate
    // for its third AND-condition; the predicate is assistant-Skill-
    // only. So a feature-branch active flow with
    // `_continue_pending=commit` and a most-recent-user-turn
    // `/flow-release` does NOT satisfy the active-flow carve-out — no
    // `flow:flow-commit` assistant Skill ran, so the choreography the
    // carve-out exists to require was skipped. Layer 10 blocks.
    //
    // Regression guard: a future edit moves the `/flow-release`
    // user-turn arm back into the shared
    // `transcript_shows_commit_window_skill` predicate, widening the
    // active-flow gate so a raw `bin/flow finalize-commit` lands on a
    // feature branch without `/flow:flow-commit` ever running.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = user_jsonl("<command-name>/flow-release</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "active-flow carve-out must NOT fire on a /flow-release user turn — no flow:flow-commit Skill ran; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn layer_10_active_flow_carveout_does_not_fire_when_flow_release_precedes_unrelated_skill() {
    // Same scoping property as the test above, with an intervening
    // non-commit assistant Skill after the `/flow-release` user turn.
    // `transcript_shows_commit_window_skill` resolves the most recent
    // assistant Skill (`flow:flow-explore`), which is not a
    // commit-window skill, so the assistant-Skill-only predicate
    // returns false and the active-flow carve-out blocks. The
    // `/flow-release` user turn is invisible to the active-flow
    // context because its recognition lives only in
    // `bootstrap_carveout_applies`.
    //
    // Regression guard: a future edit re-introduces a `/flow-release`
    // user-turn short-circuit into `transcript_shows_commit_window_skill`
    // ahead of the `most_recent_skill_since_user` check, so the
    // active-flow carve-out fires even though the model's most recent
    // action was an unrelated skill.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = format!(
        "{}{}",
        user_jsonl("<command-name>/flow-release</command-name>"),
        assistant_skill_jsonl("flow:flow-explore"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "active-flow carve-out must NOT fire when the most recent action is a non-commit Skill, even if an older user turn typed /flow-release; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
}

#[test]
fn layer_10_bootstrap_carveout_fires_for_flow_release_with_intervening_non_commit_skill() {
    // The caller-scoped counterpart to the two active-flow tests
    // above: the SAME transcript shape (`/flow-release` user turn
    // followed by an unrelated `flow:flow-explore` Skill) FIRES the
    // integration-branch bootstrap carve-out, because
    // `bootstrap_carveout_applies` recognizes the `/flow-release`
    // user turn via its `last_user_message_invokes_skill` OR-arm even
    // when `transcript_shows_commit_window_skill` returns false (the
    // most recent assistant Skill is non-commit). The bootstrap
    // window is bounded by the next real user turn, not by assistant
    // actions — so an intervening non-commit Skill during
    // `/flow-release`'s multi-step execution does not close it.
    //
    // Regression guard: a future edit drops the
    // `last_user_message_invokes_skill` OR-arm from
    // `bootstrap_carveout_applies` condition 2, so a `/flow-release`
    // bootstrap commit is blocked the moment the release skill
    // invokes any non-commit Skill before its `finalize-commit`.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        user_jsonl("<command-name>/flow-release</command-name>"),
        assistant_skill_jsonl("flow:flow-explore"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap carve-out must fire for /flow-release even with an intervening non-commit Skill; stderr={stderr}"
    );
}

// --- layer_10_trunk_carveout ---
//
// The third carve-out on Layer 10's destination-path integration-
// branch arm: when the user types `/flow:flow-commit` directly and
// the command is `bin/flow finalize-commit <trunk>`, suppress
// the block so a maintainer can commit on the trunk through
// `/flow:flow-commit`. The actual helper signature is
// `flow_commit_trunk_carveout_applies(transcript_path, home, cwd,
// main_root) -> bool`. It returns true iff:
//   1. `transcript_path` is Some (else short-circuits false).
//   2. The caller's cwd is NOT inside an active-flow worktree
//      (resolved via `detect_branch_from_path(cwd)` +
//      `is_flow_active(branch, main_root)`). When cwd IS inside an
//      active-flow worktree, the user's `/flow:flow-commit` intent
//      bound to THAT worktree's branch, not to the integration
//      trunk — the carve-out refuses to fire to prevent the
//      feature-branch-to-trunk bypass shape.
//   3. `last_user_message_invokes_skill(path, "flow:flow-commit",
//      home)` returns true — the most recent real user turn typed
//      the namespaced slash command.
//
// The command-shape precondition (`is_finalize_commit_invocation`)
// is enforced by the destination-path dispatch arm itself via
// `extract_finalize_commit_branch_arg` BEFORE the helper is called,
// so the helper doesn't re-check the shape. `git commit` shapes
// never reach the helper — they route through the cwd-path arm,
// which then fires the integration-branch block from
// `match_branch_at`.
//
// The carve-out is wired only into the destination-path integration-
// branch arm. Raw `git commit` and the cwd-path arm do NOT get the
// carve-out: a raw git invocation carries no slash-command marker
// for the gate to anchor on, and the active-flow arm has its own
// independent carve-out (assistant-Skill-only). The eight cases
// below cover the new branch shape AND prove the carve-out does not
// widen the bootstrap or active-flow paths AND does not authorize
// cross-worktree commits to the trunk.

#[test]
fn layer_10_trunk_carveout_allows_finalize_commit_when_user_typed_flow_commit() {
    // Case 1: cwd is integration branch (`main`), the transcript shows
    // a user-typed `/flow:flow-commit` slash-command turn, and the
    // command shape is `bin/flow finalize-commit main`. The
    // new trunk carve-out fires: both `is_finalize_commit_invocation`
    // and `last_user_message_invokes_skill("flow:flow-commit")` return
    // true → Layer 10 passes through. The maintainer can commit
    // directly to the trunk via `/flow:flow-commit`.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = user_jsonl("<command-name>/flow:flow-commit</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "user-typed /flow:flow-commit + finalize-commit on main must pass the trunk carve-out; stderr={stderr}"
    );
}

#[test]
fn layer_10_trunk_carveout_blocks_when_only_assistant_flow_commit_skill() {
    // Case 2: cwd is integration branch, the transcript shows ONLY an
    // assistant `flow:flow-commit` Skill (no user-typed slash command
    // for flow-commit). The trunk carve-out's second AND-condition is
    // `last_user_message_invokes_skill("flow:flow-commit")` which
    // matches only a user-typed turn shape — the assistant Skill alone
    // fails. The bootstrap carve-out also fails (no flow-start /
    // flow-prime / flow-release parent). Both carve-outs fail → block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "assistant-Skill flow:flow-commit alone must not satisfy the trunk carve-out; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_trunk_carveout_blocks_when_user_typed_unrelated_prose() {
    // Case 3: cwd is integration branch, the transcript shows an
    // unrelated user prose turn (not a slash command). Neither
    // carve-out fires — the trunk carve-out requires the user-typed
    // `<command-name>/flow:flow-commit</command-name>` shape, and a
    // free-form prose turn does not match. Block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = user_jsonl("please commit that for me");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "user prose without slash-command shape must not satisfy the trunk carve-out; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_trunk_carveout_blocks_git_commit_with_user_typed_flow_commit() {
    // Case 4: cwd is integration branch, the transcript shows a
    // user-typed `/flow:flow-commit` turn, BUT the command is `git
    // commit` rather than `bin/flow finalize-commit`. The trunk
    // carve-out is wired only into the destination-path arm; for a
    // `git commit` shape `extract_finalize_commit_branch_arg`
    // returns `None` upstream of the carve-out, so the destination-
    // path arm never fires AND the carve-out is never consulted.
    // The cwd-path arm then matches the integration branch via
    // `match_branch_at` and fires the block. Pins that the carve-out
    // is finalize-commit-only by design — there is no git-prefixed
    // escape hatch through Layer 10.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = user_jsonl("<command-name>/flow:flow-commit</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "git commit -m \"x\""}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "raw git commit must block even with user-typed /flow:flow-commit; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_trunk_carveout_blocks_when_transcript_path_missing() {
    // Case 5: cwd is integration branch, command is `bin/flow
    // finalize-commit main`, but the hook input omits
    // `transcript_path` entirely. The carve-out's
    // `let Some(path) = transcript_path else { return false }`
    // early-returns false — without a transcript, the user-typed
    // slash command cannot be verified. Block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&root), Some(&root));
    assert_eq!(
        code, 2,
        "missing transcript_path must block the trunk carve-out; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("integration branch"));
}

#[test]
fn layer_10_trunk_carveout_bootstrap_path_still_fires() {
    // Case 6 (regression): adding the trunk carve-out must not weaken
    // the bootstrap carve-out path. cwd is integration branch, the
    // transcript shows the canonical bootstrap chain Skill(flow-start)
    // followed by Skill(flow-commit), and the command is the same
    // finalize-commit shape. The bootstrap carve-out fires first → no
    // block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap carve-out path must still fire alongside the new trunk carve-out; stderr={stderr}"
    );
}

#[test]
fn layer_10_trunk_carveout_does_not_widen_active_flow_arm() {
    // Case 7 (regression): the trunk carve-out is wired only into the
    // destination-path integration-branch arm, NOT the active-flow arm.
    // Set up a feature-branch active flow with `_continue_pending=commit`
    // AND a user-typed `/flow:flow-commit` turn (no assistant Skill).
    // The active-flow arm's `transcript_shows_commit_window_skill`
    // requires an assistant Skill — the user-typed marker alone does
    // not satisfy it. Block fires. Proves the trunk carve-out's
    // user-turn recognition does not bleed into the active-flow path.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = user_jsonl("<command-name>/flow:flow-commit</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "trunk carve-out must not widen the active-flow arm — user-typed alone fails its assistant-Skill check; stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(stderr.contains("active flow"));
}

#[test]
fn layer_10_trunk_carveout_blocks_when_cwd_is_active_flow_worktree() {
    // Case 8 (Review-phase fix): the trunk carve-out's cwd-not-active-
    // flow check refuses to fire when the caller's cwd is inside an
    // active-flow worktree. Without this check, a user typing
    // `/flow:flow-commit` for a feature-branch flow could (per the
    // pre-mortem F1 bypass) authorize an unrelated `bin/flow
    // finalize-commit main` invocation from the same worktree.
    //
    // Setup: cwd is the feat worktree with an active flow; transcript
    // shows the user typed `/flow:flow-commit`; the command targets
    // `main` (the integration branch). Without the cwd-not-active-flow
    // check the trunk carve-out would suppress the integration-branch
    // block. With the check, `detect_branch_from_path(cwd)` returns
    // `"feat"`, `is_flow_active("feat", main_root)` returns true, and
    // the carve-out short-circuits false — the integration-branch
    // block fires.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = user_jsonl("<command-name>/flow:flow-commit</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "trunk carve-out must NOT fire when cwd is inside an active-flow worktree (pre-mortem F1 bypass); stderr={stderr}"
    );
    assert!(stderr.contains("BLOCKED"));
    assert!(
        stderr.contains("integration branch"),
        "block message must name the integration-branch context; got: {stderr}"
    );
}

#[test]
fn layer_10_trunk_carveout_allows_on_detached_head_with_user_typed_flow_commit() {
    // Case 9 (branch coverage): cwd is on the main repo with HEAD
    // detached, AND the transcript shows a user-typed
    // `/flow:flow-commit` turn. `detect_branch_from_path(cwd)` returns
    // `None` (no `.worktrees/` marker AND `git branch --show-current`
    // returns empty under detached HEAD). The cwd-not-active-flow
    // check's `if let Some(branch) = …` short-circuits with no body
    // execution — the carve-out falls through to the walker and fires.
    // This pins the branch where `detect_branch_from_path` returns
    // `None`. Acceptable behavior: there is no detectable active-flow
    // worktree at cwd to constrain the carve-out to, so the user's
    // typed `/flow:flow-commit` authorization stands.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();
    // Detach HEAD so `git branch --show-current` returns empty and
    // `detect_branch_from_path` returns `None` (matches t17's
    // detach mechanism).
    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .expect("git rev-parse");
    let sha = String::from_utf8_lossy(&rev.stdout).trim().to_string();
    let _ = Command::new("git")
        .args(["update-ref", "--no-deref", "HEAD", &sha])
        .current_dir(&root)
        .output()
        .expect("detach HEAD");

    let jsonl = user_jsonl("<command-name>/flow:flow-commit</command-name>");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "detached HEAD + user-typed /flow:flow-commit on main must allow via the trunk carve-out; stderr={stderr}"
    );
}

#[test]
fn layer_10_trunk_carveout_block_message_points_at_trunk_path() {
    // Block-message reword: the integration-branch block message
    // now points the maintainer at the supported on-trunk path
    // (`/flow:flow-commit` on the trunk branch) rather than at a
    // feature worktree. Preserves the `BLOCKED` prefix and the
    // interpolated branch name so existing content-presence
    // assertions stay green; adds the new "trunk branch" pointer so
    // a maintainer with a legitimate trunk commit need is told how
    // to proceed.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    // No transcript supplied — both carve-outs fail, the block fires
    // and emits the reworded message.
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&root), Some(&root));
    assert_eq!(code, 2, "no transcript must block; stderr={stderr}");
    assert!(stderr.contains("BLOCKED"));
    assert!(
        stderr.contains("integration branch 'main'"),
        "reworded block message must still name the canonical integration branch; got: {stderr}"
    );
    assert!(
        stderr.contains("/flow:flow-commit"),
        "reworded block message must redirect to /flow:flow-commit; got: {stderr}"
    );
    assert!(
        stderr.contains("trunk branch"),
        "reworded block message must name the supported on-trunk path; got: {stderr}"
    );
}

// --- layer_10_finalize_commit_destination ---
//
// Layer 10's destination-aware dispatch fires when the command
// shape is `bin/flow finalize-commit <branch>`. The routing key is
// the explicit branch argument (the first positional after the
// subcommand), not the caller's process cwd. The tests below cover:
//
//   - extract_finalize_commit_branch_arg: per-branch behavior of
//     the new parser (Task 8 sibling tests).
//   - match_finalize_commit_destination: integration-branch match
//     vs. feature-branch miss (Tasks 9, 10).
//   - End-to-end cwd-independence (Tasks 11, 12).
//   - Carve-out interactions (Tasks 13, 14).
//   - Regression tests proving non-finalize-commit shapes still
//     route through the cwd path (Tasks 15, 16).
//
// Mirror the production binding: the binary at
// `src/finalize_commit.rs::run_impl` derives `commit_cwd` from the
// branch arg via `FlowPaths::worktree()`. The hook must agree on
// the destination so a block on the hook side and a successful
// commit on the binary side cannot land on different branches.

#[test]
fn extract_finalize_commit_branch_arg_returns_none_for_git_commit() {
    // `git commit` is not a `bin/flow` invocation → the new
    // dispatch's first token check returns None. Falls through to
    // the existing cwd path → match_branch_at(main) blocks.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git commit -F msg.txt"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "git commit on main must block; stderr={stderr}");
    assert!(stderr.contains("integration branch"));
}

#[test]
fn extract_finalize_commit_branch_arg_returns_none_for_bin_flow_status() {
    // `bin/flow status` is not a commit invocation at all →
    // is_commit_invocation returns false → the new dispatch is
    // never consulted. Layer 10 silent → allow.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow status"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 0, "bin/flow status must allow; stderr={stderr}");
}

#[test]
fn extract_finalize_commit_branch_arg_returns_none_for_legacy_two_positional() {
    // Legacy two-positional shape `bin/flow finalize-commit <path> <branch>`:
    // the first token after `finalize-commit` is now the branch
    // candidate, but a path-like token containing `/` fails
    // `is_valid_branch`, so the parser returns None and the dispatch
    // falls through to the cwd-based check. With cwd on the
    // integration branch, the cwd path blocks — proving the
    // fall-through happened rather than a destination match on the
    // path-like token.
    let (_dir, root) = setup_repo_on_branch("main");
    let input =
        r#"{"tool_input": {"command": "bin/flow finalize-commit .flow-states/feat/msg.txt feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "legacy path-like first token must fall through to cwd path and block on integration; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
}

#[test]
fn extract_finalize_commit_branch_arg_returns_branch_for_happy_path() {
    // `bin/flow finalize-commit main` parses to
    // Some("main"). main_root resolves via the .claude/ fixture
    // and default_branch_in falls back to "main", so the
    // destination match fires and blocks the call from a sibling
    // worktree cwd — proving the parser returned the branch arg.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "branch arg main must block via destination check; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
    assert!(stderr.contains("main"));
}

#[test]
fn extract_finalize_commit_branch_arg_returns_none_for_missing_branch_token() {
    // `bin/flow finalize-commit` has no positional branch token, so
    // `extract_finalize_commit_branch_arg` returns None. The dispatch
    // falls through to the existing cwd path → match_branch_at
    // resolves "main" from the repo's branch.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "missing branch token falls through to cwd path; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
}

#[test]
fn extract_finalize_commit_branch_arg_dequotes_branch_token() {
    // `bin/flow finalize-commit "main"` should dequote
    // the branch token to "main" — the destination match fires
    // even though the raw token is `"main"` with quotes.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit \"main\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "quoted branch arg main must block via destination check; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
    assert!(stderr.contains("main"));
}

#[test]
fn match_finalize_commit_destination_blocks_integration_branch_arg() {
    // Default integration branch is "main" (no remote configured
    // for the worktree fixture). branch_arg="main" matches →
    // block with the integration-branch message. Includes the
    // case-variant input to prove normalize_gate_input runs on
    // both sides.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit MAIN"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "case-variant integration branch arg must block; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
}

/// Delegation contract (Task 6): `match_finalize_commit_destination`
/// computes the integration-branch decision via
/// `crate::flow_paths::finalize_commit_destination` but still names
/// the CANONICAL integration branch (resolved through
/// `default_branch_in`) in the block message — not the raw
/// case-variant `branch_arg`. Regression guard for the gate-action-
/// atomicity contract: if the delegation ever named `branch_arg`
/// instead, a `MAIN` arg would surface `MAIN` to the user. The arg
/// is `MAIN`; the message must read `integration branch 'main'` and
/// must not echo the raw `MAIN`.
#[test]
fn finalize_commit_destination_block_message_names_canonical_integration_branch() {
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit MAIN"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "case-variant integration arg must block via delegated destination; stderr={stderr}"
    );
    assert!(
        stderr.contains("integration branch 'main'"),
        "block message must name canonical integration branch 'main'; got: {stderr}"
    );
    assert!(
        !stderr.contains("MAIN"),
        "block message must not echo the raw case-variant arg 'MAIN'; got: {stderr}"
    );
}

#[test]
fn match_finalize_commit_destination_allows_feature_branch_arg() {
    // branch_arg="some-feature" does NOT match the integration
    // branch ("main" fallback) → integration arm returns None.
    // worktree_path = <root>/.worktrees/some-feature/ has no
    // active state file → active-flow arm returns None. Allow.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit some-feature"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "feature-branch arg with no active flow must allow; stderr={stderr}"
    );
}

/// Hook/binary destination-agreement contract (Task 7). Layer 10's
/// integration-branch arm must fire exactly when the shared helper
/// `finalize_commit_destination` routes the commit to the project
/// root — the structural invariant #1660 demands so the hook's
/// block decision and the binary's commit cwd cannot disagree. For
/// a fixture repo with `origin/HEAD` → `main` and no active flow on
/// the test branches, the hook's only firing path for a
/// `bin/flow finalize-commit <branch>` invocation is the
/// integration-branch destination arm, so "hook blocks via the
/// destination path" ⟺ "the helper routes to root". The
/// whitespace-padded `" main "` arm of the helper's normalize-both-
/// sides contract cannot be carried as a single shell token through
/// the command parser (split_whitespace would shatter it), so that
/// element is asserted against the pub helper directly — which is
/// the exact decision `match_finalize_commit_destination` delegates
/// to, so the agreement still holds transitively.
#[test]
fn hook_blocks_finalize_commit_iff_helper_routes_to_project_root() {
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    for b in ["main", "MAIN", "feat-x"] {
        let dest_is_root = finalize_commit_destination(&root, b) == root;
        let input = format!(r#"{{"tool_input": {{"command": "bin/flow finalize-commit {b}"}}}}"#);
        let (code, _stdout, stderr) = run_hook_with_input(&input, Some(&root));
        let hook_blocks_via_destination = code == 2 && stderr.contains("integration branch");
        assert_eq!(
            dest_is_root, hook_blocks_via_destination,
            "branch {b:?}: finalize_commit_destination==root is {dest_is_root} but \
             hook-blocks-via-destination is {hook_blocks_via_destination}; stderr={stderr}"
        );
    }

    // Whitespace-padded arg: not expressible as a single shell token,
    // so assert the shared helper (the decision the hook delegates
    // to) routes it to the project root.
    assert_eq!(
        finalize_commit_destination(&root, " main "),
        root,
        "normalize-both-sides: ' main ' must route to the project root"
    );
}

#[test]
fn finalize_commit_invocation_with_integration_branch_arg_blocks_regardless_of_cwd() {
    // Cwd is a feature-branch worktree (so the OLD cwd path would
    // have resolved feat ≠ main → None and allowed the call
    // through). The NEW destination path resolves branch_arg=main
    // and blocks. Proves the destination check fires regardless
    // of where the caller's shell sits — the regression-protection
    // contract.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "integration branch arg must block from feature worktree cwd; stderr={stderr}"
    );
    assert!(stderr.contains("integration branch"));
}

#[test]
fn finalize_commit_invocation_with_feature_branch_arg_allows_regardless_of_cwd() {
    // Cwd is on the integration branch (so the OLD cwd path would
    // have blocked via match_branch_at(main)). The NEW destination
    // path resolves branch_arg=feature-foo, which differs from
    // integration, returns None for the integration arm; the
    // active-flow arm checks at <root>/.worktrees/feature-foo/
    // which has no state file → None. Allow. Proves the
    // destination check honours the branch arg even when cwd
    // would have blocked under the old behavior.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit feature-foo"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "feature branch arg must allow from integration cwd; stderr={stderr}"
    );
}

#[test]
fn bootstrap_carveout_fires_for_finalize_commit_integration_branch_arg_with_sanctioned_chain() {
    // Cwd is on integration. Transcript shows
    // Skill(flow:flow-start) + Skill(flow:flow-commit) since the
    // most recent user turn — the canonical bootstrap window.
    // The new destination check would block (branch_arg=main
    // matches integration), but bootstrap_carveout_applies fires
    // and suppresses it. No active-flow at
    // <root>/.worktrees/main/ → final result is allow.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap carve-out must suppress the destination check on the integration branch arg; stderr={stderr}"
    );
}

#[test]
fn active_flow_carveout_fires_for_finalize_commit_feature_branch_arg_with_flow_commit_skill() {
    // Active-flow carve-out via the new destination dispatch.
    // worktree_path = <root>/.worktrees/feat/, state file has
    // _continue_pending="commit", transcript shows
    // Skill(flow:flow-commit) — all three carve-out conditions
    // hold → allow. Inverse: without the transcript fixture, the
    // walker condition fails and the active-flow block fires.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let allow_input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (allow_code, _stdout, allow_stderr) =
        run_hook_with_input_and_home(&allow_input, Some(&cwd), Some(&root));
    assert_eq!(
        allow_code, 0,
        "active-flow carve-out must fire on feature branch arg with flow-commit Skill; stderr={allow_stderr}"
    );

    // Inverse: no transcript → walker condition fails → block.
    let block_input = r#"{"tool_input": {"command": "bin/flow finalize-commit feat"}}"#;
    let (block_code, _stdout, block_stderr) = run_hook_with_input(block_input, Some(&cwd));
    assert_eq!(
        block_code, 2,
        "without transcript fixture the active-flow block must fire; stderr={block_stderr}"
    );
    assert!(block_stderr.contains("active flow"));
}

#[test]
fn cwd_path_bootstrap_carveout_fires_when_finalize_commit_lacks_branch_arg() {
    // Coverage-required: the cwd path's bootstrap-carve-out arm is
    // reachable only for finalize-commit invocations where the branch
    // arg cannot be extracted (so the new destination dispatch falls
    // through). Setup: cwd=main, command omits the branch positional,
    // transcript shows the canonical bootstrap chain (flow-start +
    // flow-commit). The carve-out fires, no block.
    let (_dir, root) = setup_repo_on_branch("main");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let jsonl = format!(
        "{}{}",
        assistant_skill_jsonl("flow:flow-start"),
        assistant_skill_jsonl("flow:flow-commit"),
    );
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    // No branch positional — extract_finalize_commit_branch_arg
    // returns None because tokens.next() after `finalize-commit` is
    // None.
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "cwd-path bootstrap carve-out must fire for finalize-commit shape without branch arg; stderr={stderr}"
    );
}

#[test]
fn plain_git_commit_still_uses_cwd_match_branch_at() {
    // Regression: plain `git commit` from cwd=main must continue
    // to block via the existing cwd path (match_branch_at(cwd)).
    // Pins the boundary so a future refactor that accidentally
    // migrates non-finalize-commit shapes onto the new destination
    // check fails CI.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 2,
        "plain git commit on main must block via cwd path; stderr={stderr}"
    );
    assert!(
        stderr.contains("integration branch"),
        "must use integration-branch message; got: {stderr}"
    );
}

#[test]
fn git_dash_c_path_still_uses_target_match_branch_at() {
    // Regression: `git -C <main_repo_path> commit` from a feature
    // worktree must continue to block via the cwd path's `-C`
    // target check. extract_finalize_commit_branch_arg returns
    // None for git commands, so the dispatch falls through. The
    // -C target's match_branch_at fires.
    let (_main_dir, main_root) = setup_repo_on_branch("main");
    let (_feat_dir, feat_root) = setup_repo_on_branch("feat-x");
    let main_path = main_root.to_str().expect("utf-8 main path");
    let cmd = format!(
        r#"{{"tool_input": {{"command": "git -C {} commit -m \"x\""}}}}"#,
        main_path
    );
    let (code, _stdout, stderr) = run_hook_with_input(&cmd, Some(&feat_root));
    assert_eq!(
        code, 2,
        "git -C <main_path> commit must block via target match_branch_at; stderr={stderr}"
    );
    assert!(
        stderr.contains("main"),
        "must name the -C target branch 'main'; got: {stderr}"
    );
}

// --- Layer 9 returns None on default_branch_in resolve failure ---
//
// When git cannot resolve the integration branch (no `origin` remote,
// symbolic-ref unset, non-git directory), Layer 9 has no basis to
// fire on an integration-branch destination — the integration
// branch is undetectable, so a feature-branch commit cannot be
// matched against it. The gate returns None (no block) so commits
// in unprimed/fresh-clone repos proceed. The active-flow check at
// the same callsite is independent and still catches in-flow
// commits via the state file. See
// `match_finalize_commit_destination` and `match_branch_at` in
// src/hooks/validate_pretool.rs.

#[test]
fn layer_9_returns_none_on_finalize_commit_destination_when_default_branch_resolve_fails() {
    // Tempdir with no git init — `default_branch_in` returns Err.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    // No flow active and no integration branch detectable — gate
    // returns None, hook exits 0 (no block).
    assert_eq!(
        code, 0,
        "destination-path dispatch must return no block when integration branch is undetectable; stderr={stderr}"
    );
    assert!(
        !stderr.contains("BLOCKED"),
        "stderr must not contain BLOCKED when no integration-branch match is possible; got: {stderr}"
    );
}

#[test]
fn layer_9_returns_none_on_commit_when_default_branch_resolve_fails_cwd_path() {
    // Tempdir with no origin/HEAD — match_branch_at's
    // default_branch_in returns Err and the cwd-path dispatch
    // returns None (no integration-branch block). The user is on a
    // feature branch and the integration branch is undetectable;
    // the gate has no basis to fire.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let claude_dir = root.join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), "{}").unwrap();
    // Init repo so current_branch_in succeeds, but no origin/HEAD.
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "t@t.com"])
        .current_dir(&root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "T"])
        .current_dir(&root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&root)
        .output();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "feature-x"])
        .current_dir(&root)
        .output();

    // git commit on feature-x with no origin/HEAD — cwd-path dispatch
    // must NOT block (no integration branch detectable).
    let input = r#"{"tool_input": {"command": "git commit -m \"x\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "cwd-path dispatch must return no block when integration branch is undetectable; stderr={stderr}"
    );
    assert!(
        !stderr.contains("BLOCKED"),
        "stderr must not contain BLOCKED on feature-branch commit when origin/HEAD is unset; got: {stderr}"
    );
}

// --- halt gate ---
//
// `_halt_pending=true` in the state file refuses every model-
// initiated flow-advancing Bash command. The closed allowlist
// targets the exact subcommand shapes that progress the autonomous
// flow past the user's halt directive: code-task counter increment,
// phase entry / completion / transition, the commit finalize, and
// the per-session utility marker. Non-advancing `bin/flow`
// subcommands (logging, status, set-timestamp on non-counter
// fields) and arbitrary other Bash commands pass through the gate.
//
// `/flow:flow-continue` invokes `bin/flow clear-halt` to clear
// `_halt_pending`. `clear-halt` is NOT in the advancing-commands
// list (its purpose IS to exit the halt) and falls through cleanly.

#[test]
fn validate_pretool_blocks_set_timestamp_code_task_during_halt() {
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --set code_task=5"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "set-timestamp code_task must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("/flow:flow-continue"),
        "block message must name /flow:flow-continue: {stderr}"
    );
    assert!(stderr.contains("/flow:flow-abort"));
}

#[test]
fn validate_pretool_blocks_phase_enter_during_halt() {
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow phase-enter --phase flow-code"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(code, 2, "phase-enter must block; stderr={stderr}");
    assert!(stderr.contains("/flow:flow-continue"));
}

#[test]
fn validate_pretool_blocks_phase_finalize_during_halt() {
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input =
        r#"{"tool_input": {"command": "bin/flow phase-finalize --phase flow-code --branch feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(code, 2, "phase-finalize must block; stderr={stderr}");
    assert!(stderr.contains("/flow:flow-continue"));
}

#[test]
fn validate_pretool_blocks_phase_transition_during_halt() {
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow phase-transition --action complete"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(code, 2, "phase-transition must block; stderr={stderr}");
    assert!(stderr.contains("/flow:flow-continue"));
}

#[test]
fn validate_pretool_blocks_finalize_commit_during_halt() {
    // finalize-commit advances the flow past the halt. Even when
    // `_continue_pending=commit` is also set (which would normally
    // satisfy Layer 10's active-flow carve-out), the halt gate runs
    // AFTER Layer 10 and refuses. The user must clear the halt via
    // `/flow:flow-continue` before commits can resume.
    let (_dir, root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"_halt_pending": true, "_continue_pending": "commit"}"#,
    );
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "finalize-commit must block during halt; stderr={stderr}"
    );
    assert!(stderr.contains("/flow:flow-continue"));
}

#[test]
fn validate_pretool_blocks_set_utility_in_progress_during_halt() {
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input =
        r#"{"tool_input": {"command": "bin/flow set-utility-in-progress --skill flow:flow-plan"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "set-utility-in-progress must block; stderr={stderr}"
    );
    assert!(stderr.contains("/flow:flow-continue"));
}

#[test]
fn validate_pretool_allows_clear_halt_when_transcript_shows_continue_command() {
    // `bin/flow clear-halt` is the user's resume action. The
    // command is not in `is_flow_advancing_bash_command`'s
    // allowlist (its purpose IS to exit the halt), so the halt
    // gate passes through. `clear-halt::run_impl` itself self-
    // gates on the transcript via `last_user_message_invokes_skill`
    // — the only sanctioned caller is `/flow:flow-continue`.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow clear-halt --branch feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "clear-halt must pass the halt gate (its purpose is to exit the halt); stderr={stderr}"
    );
}

#[test]
fn validate_pretool_allows_set_timestamp_non_code_task_field_during_halt() {
    // `--set code_task_name=...` is non-advancing — TUI display
    // only, no counter mutation. The halt gate must let it
    // through so the model can keep state metadata accurate even
    // while the flow is paused. (Mainly defensive — the model
    // shouldn't be writing state fields during halt at all, but
    // the gate is allowlist-based and only blocks the closed set
    // of advancing commands.)
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --set code_task_name=\"resuming\""}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "set-timestamp code_task_name must pass (non-counter field); stderr={stderr}"
    );
}

#[test]
fn validate_pretool_allows_flow_advancing_bash_when_halt_not_set() {
    // No halt → counter-advancing command passes. Confirms the
    // halt gate only fires when `_halt_pending=true`.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": false}"#);
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --set code_task=5"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "set-timestamp code_task must pass when halt not set; stderr={stderr}"
    );
}

#[test]
fn validate_pretool_blocks_set_timestamp_code_task_equals_form_during_halt() {
    // CLI argument tokenization can deliver `--set` and `code_task=N`
    // as a single combined token `--set=code_task=N`. The halt-gate
    // allowlist must recognize both spacing variants — splitting on
    // `--set ` alone leaves the equals-form as a bypass surface that
    // advances the flow past the user's halt directive.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --set=code_task=5"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "set-timestamp --set=code_task=N equals-form must block; stderr={stderr}"
    );
    assert!(
        stderr.contains("/flow:flow-continue"),
        "block message must name /flow:flow-continue: {stderr}"
    );
}

#[test]
fn validate_pretool_halt_gate_fires_when_settings_json_missing() {
    // The halt gate must NOT depend on `.claude/settings.json` to
    // resolve the branch and main_root. Settings is consulted only
    // for Layer 9 whitelist enforcement; conflating that with halt
    // detection silently disables the halt gate in environments
    // where settings.json is absent (interrupted prime, CI runners
    // that gitignore it, fresh clones before /flow:flow-prime). The
    // active-flow state file at
    // `<main_root>/.flow-states/<branch>/state.json` is the
    // authoritative signal, derived from cwd alone.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_halt_pending": true}"#);
    // Remove settings.json to simulate the pre-prime / CI scenario.
    std::fs::remove_file(root.join(".claude").join("settings.json")).unwrap();
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --set code_task=5"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 2,
        "halt gate must block flow-advancing commands even without settings.json; stderr={stderr}"
    );
    assert!(
        stderr.contains("/flow:flow-continue"),
        "block message must name /flow:flow-continue: {stderr}"
    );
}

#[test]
fn finalize_commit_destination_arm_falls_through_when_project_root_missing() {
    // Covers the destination-path arm in `check_commit_during_flow`
    // where `extract_finalize_commit_branch_arg` returns Some but
    // `find_settings_and_root_from(cwd)` returns (_, None) — no
    // `.claude/settings.json` in any ancestor of cwd. Without a
    // project root, the destination check cannot resolve the
    // integration branch and the arm falls through to the cwd path
    // (which is also a no-op since the cwd has no git repo). The
    // hook returns exit 0 (allow).
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    // Fresh tempdir with no .claude/ and no .git/. cwd ancestors
    // have no settings.json (tempdir lives under /var/folders/ on
    // macOS, /tmp/ on Linux — neither contains .claude/settings.json).

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit main"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(
        code, 0,
        "finalize-commit from a no-project-root cwd must fall through \
         without blocking; stderr={stderr}"
    );
}

// --- layer_11_ci_during_code_phase ---
//
// Layer 11 redirects `bin/flow ci` to the per-file gate during Code
// phase. The full-CI runner is wasteful for single-file iteration
// when `bin/test tests/<name>.rs` enforces identical 100/100/100
// thresholds at seconds-scale. The single carve-out is
// `bin/flow ci --clean` (the documented phantom-misses recovery
// path). `finalize_commit::run_impl` calls `ci::run_impl()` as a
// Rust function and never reaches this Bash hook — the commit-time
// CI gate is structurally unaffected.
//
// The fixture `setup_active_flow_worktree_with_state` (defined
// above) builds the minimal layout: settings.json,
// `.flow-states/<branch>/state.json` with caller-controlled content,
// and a `.worktrees/<branch>/` directory with a `.git` pointer so
// `detect_branch_from_path` resolves. The Code-phase state shape
// the gate looks for is `current_phase == "flow-code"` AND
// `phases.flow-code.status == "in_progress"`.

const CODE_PHASE_STATE: &str =
    r#"{"current_phase": "flow-code", "phases": {"flow-code": {"status": "in_progress"}}}"#;

/// Assert the Layer 11 block fires: exit code 2, stderr names the
/// redirect target and the per-file rule.
fn assert_layer_11_block(code: i32, stderr: &str, context: &str) {
    assert_eq!(code, 2, "{context}: must block; stderr={stderr}");
    assert!(
        stderr.contains("BLOCKED"),
        "{context}: stderr should contain BLOCKED; got: {stderr}"
    );
    assert!(
        stderr.contains("bin/test tests/"),
        "{context}: stderr should redirect to per-file gate; got: {stderr}"
    );
    assert!(
        stderr.contains("per-file-coverage-iteration.md"),
        "{context}: stderr should cite the per-file rule; got: {stderr}"
    );
}

// Block-fires set: every `bin/flow ci` shape during Code phase
// produces the Layer 11 block.

#[test]
fn layer_11_blocks_bare_bin_flow_ci_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bare bin/flow ci");
}

#[test]
fn layer_11_blocks_bin_flow_ci_test_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --test"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --test");
}

#[test]
fn layer_11_blocks_bin_flow_ci_audit_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --audit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --audit");
}

#[test]
fn layer_11_blocks_bin_flow_ci_build_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --build"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --build");
}

#[test]
fn layer_11_blocks_bin_flow_ci_force_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --force"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --force");
}

#[test]
fn layer_11_blocks_bin_flow_ci_format_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --format"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --format");
}

#[test]
fn layer_11_blocks_bin_flow_ci_lint_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --lint"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --lint");
}

#[test]
fn layer_11_blocks_absolute_path_bin_flow_ci_in_code_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "/Users/x/code/flow/bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "absolute-path bin/flow ci");
}

// Carve-out set: `--clean` lets the command through so the
// documented phantom-misses recovery path stays available.

#[test]
fn layer_11_carveout_allows_bin_flow_ci_clean() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --clean"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(code, 0, "--clean must carve out; stderr={stderr}");
}

#[test]
fn layer_11_carveout_allows_bin_flow_ci_clean_with_branch_arg() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --clean --branch foo"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "--clean with branch arg must carve out; stderr={stderr}"
    );
}

// Pass-through set: every non-Code-phase context allows
// `bin/flow ci` through.

#[test]
fn layer_11_does_not_fire_when_no_active_flow() {
    // No state.json at all → is_flow_active returns false → Layer 11
    // never reaches state_is_in_code_phase.
    let (_dir, _root, cwd) = setup_active_flow_worktree("feat", false);
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "no active flow must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_in_flow_start_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-start", "phases": {"flow-code": {"status": "pending"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-start phase must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_in_flow_review_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-review", "phases": {"flow-review": {"status": "in_progress"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-review phase must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_in_flow_learn_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-learn", "phases": {"flow-learn": {"status": "in_progress"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-learn phase must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_in_flow_complete_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-complete", "phases": {"flow-complete": {"status": "in_progress"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-complete phase must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_when_code_status_pending() {
    // current_phase is flow-code but the phase status hasn't reached
    // in_progress yet (e.g. between phase_complete and phase_enter).
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": {"flow-code": {"status": "pending"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-code status=pending must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_when_code_status_complete() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": {"flow-code": {"status": "complete"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "flow-code status=complete must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_for_bin_flow_status_subcommand() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow status"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "bin/flow status must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_for_bin_flow_phase_transition_subcommand() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow phase-transition --action complete"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "bin/flow phase-transition must not trip Layer 11; stderr={stderr}"
    );
}

// Fail-closed set: every state-file corruption shape returns no
// block (the friction-prevention inversion of Layer 10's posture).

#[test]
fn layer_11_no_block_when_state_unparseable() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", "{this is not json}");
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "unparseable state.json must allow bin/flow ci (fail-closed-no-block); stderr={stderr}"
    );
}

#[test]
fn layer_11_no_block_when_current_phase_absent() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"phases": {"flow-code": {"status": "in_progress"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "absent current_phase must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_no_block_when_phases_wrong_type() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": 42}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "phases=number must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_no_block_when_flow_code_entry_wrong_type() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": {"flow-code": "in_progress"}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "phases.flow-code=string must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_no_block_when_code_status_wrong_type() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": {"flow-code": {"status": 1}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "status=number must allow bin/flow ci; stderr={stderr}"
    );
}

#[cfg(unix)]
#[test]
fn layer_11_no_block_when_state_file_is_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    // `is_flow_active`'s `.is_file()` succeeds even when the file's
    // read perms are 000 — metadata is fetched from the parent dir,
    // not by reading content. The downstream `state_is_in_code_phase`
    // then attempts `read_to_string`, which returns `Err(EACCES)`.
    // Fail-closed-as-no-block (the Layer 11 inversion of Layer 10's
    // posture): the read failure means we can't confirm Code phase,
    // so `bin/flow ci` is allowed through.
    let (_dir, root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let state_path = root.join(".flow-states").join("feat").join("state.json");

    let mut perms = std::fs::metadata(&state_path).unwrap().permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&state_path, perms).unwrap();

    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));

    // Restore perms before any assertion can short-circuit tempdir
    // cleanup.
    let mut perms = std::fs::metadata(&state_path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&state_path, perms).unwrap();

    assert_eq!(
        code, 0,
        "unreadable state.json must allow bin/flow ci; stderr={stderr}"
    );
}

#[test]
fn layer_11_no_block_when_state_file_has_invalid_utf8() {
    // Drives the `read_state_file_capped` `read_to_string` Err
    // branch — `File::open` succeeds (state.json exists and is
    // readable) but the content is not valid UTF-8. Fail-closed-
    // as-no-block: the gate falls through and `bin/flow ci`
    // passes.
    let (_dir, root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let state_path = root.join(".flow-states").join("feat").join("state.json");
    std::fs::write(&state_path, [0xFF, 0xFE, 0xFD]).unwrap();
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "invalid UTF-8 in state.json must allow bin/flow ci; stderr={stderr}"
    );
}

// Subcommand-position discipline: `ci` must appear as the first
// non-flag token after `bin/flow`. Sibling subcommands whose args
// happen to include the literal `ci` token must NOT trip Layer 11.

#[test]
fn layer_11_does_not_fire_for_phase_enter_with_phase_arg_ci() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow phase-enter --phase ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "phase-enter --phase ci must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_for_phase_transition_with_action_ci() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow phase-transition --action ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "phase-transition --action ci must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_for_log_with_ci_in_message() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow log feat Phase-2 ci notes"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "log with bare ci in message must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_for_set_timestamp_with_value_ci() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow set-timestamp --field ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "set-timestamp with --field ci must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_does_not_fire_when_only_global_flags_no_subcommand() {
    // Drives the `is_flow_ci_invocation` "ran out of tokens" path —
    // first token passes `is_bin_flow_token`, every subsequent token
    // is a global flag or its value, no non-flag subcommand surfaces.
    // Per UNIVERSAL_ALLOW, `Bash(*bin/flow *)` passes Layer 9.
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow --log-level info --foo bar"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "global flags only must not trip Layer 11; stderr={stderr}"
    );
}

#[test]
fn layer_11_blocks_with_global_flag_value_pair_before_ci() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow --log-level info ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow --log-level info ci");
}

#[test]
fn layer_11_blocks_with_global_flag_equals_value_before_ci() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow --log-level=info ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow --log-level=info ci");
}

// Carve-out normalization: case variants and the `--flag=value`
// form both reach the recovery path per
// `.claude/rules/security-gates.md` "Normalize Before Comparing".

#[test]
fn layer_11_carveout_allows_uppercase_clean_flag() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --CLEAN"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 0,
        "--CLEAN case variant must carve out; stderr={stderr}"
    );
}

#[test]
fn layer_11_carveout_allows_clean_with_equals_value() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --clean=true"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(code, 0, "--clean=true must carve out; stderr={stderr}");
}

#[test]
fn layer_11_blocks_no_clean_variant() {
    // `--no-clean` is a distinct flag, not the carve-out token.
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state("feat", CODE_PHASE_STATE);
    let input = r#"{"tool_input": {"command": "bin/flow ci --no-clean"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "bin/flow ci --no-clean");
}

// Normalize before comparing applies to state-file values too:
// hand-edited or legacy state files with case- or whitespace-variant
// `current_phase` / `phases.flow-code.status` strings still trigger
// the gate per `.claude/rules/security-gates.md`.

#[test]
fn layer_11_fires_on_case_variant_current_phase() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "FLOW-CODE", "phases": {"flow-code": {"status": "in_progress"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "case-variant current_phase");
}

#[test]
fn layer_11_fires_on_case_variant_status() {
    let (_dir, _root, cwd) = setup_active_flow_worktree_with_state(
        "feat",
        r#"{"current_phase": "flow-code", "phases": {"flow-code": {"status": "IN_PROGRESS"}}}"#,
    );
    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_layer_11_block(code, &stderr, "case-variant status");
}

#[test]
fn layer_11_no_block_when_no_project_root() {
    // Build a fixture where `.worktrees/<branch>/` exists so
    // `detect_branch_from_path` returns Some — but no
    // `.claude/settings.json` exists anywhere upward, so
    // `find_settings_and_root_from` returns `(None, None)` and the
    // `let root = project_root?` line returns None. Layer 11 passes
    // through.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonicalize");
    let worktree = root.join(".worktrees").join("feat");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/feat").unwrap();
    // Deliberately omit `.claude/settings.json` — find_settings walks
    // up to filesystem root without finding it.

    let input = r#"{"tool_input": {"command": "bin/flow ci"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&worktree));
    assert_eq!(
        code, 0,
        "no project root must allow bin/flow ci; stderr={stderr}"
    );
}
