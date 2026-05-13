//! Integration tests for `src/hooks/validate_pretool.rs`.

use std::io::Write;
use std::process::{Command, Stdio};

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
// whitelist (Layer 8) can permit them via UNIVERSAL_ALLOW's
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

// --- Layer 7.5: structural escape-hatch program/flag block ---
//
// Layer 7.5 in src/hooks/validate_pretool.rs::validate strips env-var
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

// --- Layer 7.5: indirect-form rejections ---
//
// Glob deny patterns require the exact first-token spelling; these
// indirect shapes (absolute path prefix, env-var prefix, flags
// before the trigger) route around Layer 7's settings-driven check.
// Layer 7.5's structural tokenization catches them.

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

// --- Layer 7.5: pass-through cases ---
//
// `bash -n script.sh` (syntax check, no eval) is in UNIVERSAL_ALLOW
// and must pass Layer 7.5 untouched. `ssh-keygen` has the basename
// `ssh-keygen` rather than `ssh` and must NOT trip the ssh-class
// block — basename matching is exact, not prefix-based.

#[test]
fn test_layer_7_5_passes_bash_dash_n() {
    let (allowed, msg) = validate("bash -n script.sh", None, false);
    assert!(allowed, "bash -n must pass Layer 7.5; msg={msg:?}");
    assert!(msg.is_empty());
}

#[test]
fn test_layer_7_5_passes_ssh_keygen() {
    // Pass settings=None so we skip Layer 8's whitelist; the test is
    // that Layer 7.5 doesn't fire on `ssh-keygen`.
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
    // shape, so Layer 7.5 must let it through.
    let (allowed, _msg) = validate("tmux ls", None, false);
    assert!(allowed, "tmux without send-keys subcommand must pass");
}

#[test]
fn test_layer_7_5_passes_screen_ls() {
    // `screen -ls` lists sessions — not the `-X` stuff-key shape, so
    // Layer 7.5 must let it through.
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
    // escape-hatch program — Layer 7.5 returns None and the call
    // passes through.
    let (allowed, _msg) = validate("FOO=BAR", None, false);
    assert!(allowed);
}

// --- Layer 7.5: combined-flag scan (adversarial regression) ---
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

// --- Layer 7.5: wrapper-launcher strip (adversarial regression) ---
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
    // `first` extraction fails, and Layer 7.5 returns None.
    let (allowed, _msg) = validate("env", None, false);
    assert!(allowed);
}

#[test]
fn test_layer_7_5_passes_time_alone() {
    let (allowed, _msg) = validate("time", None, false);
    assert!(allowed);
}

// --- Layer 7.5: additional interpreter-eval programs ---
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

// --- Layer 7.5: tmux with global flags (adversarial regression) ---
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

// --- Layer 7.5: block message sanctioned-alternative content ---

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
    let msg = should_block_background("bin/flow finalize-commit .flow-commit-msg main", false);
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
    let msg = should_block_background(
        "/Users/ben/code/flow/bin/flow finalize-commit .flow-commit-msg main",
        false,
    );
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_flow-rs"))
        .args(["hook", "validate-pretool"])
        .env_remove("FLOW_CI_RUNNING")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn flow-rs");
    {
        let stdin = child.stdin.as_mut().unwrap();
        let input = json!({
            "tool_input": {
                "command": "bin/flow ci",
                "run_in_background": bg,
            }
        });
        stdin
            .write_all(serde_json::to_string(&input).unwrap().as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
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
// Layer 9: block direct commit invocations when the hook's effective
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
    (dir, root)
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
    // fallback (no remote configured). Layer 9 does not fire.
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
    // message names the staging branch — proving Layer 9 honours the
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
    // Layer 9 only fires on `git ... commit`. `git status` is a
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
    // 7.5 (`.claude/rules/no-escape-hatches.md` "Canonical
    // Escape-Hatch Shapes"). Layer 9's commit-invocation matcher
    // never sees the wrapped `git commit` because Layer 7.5 fires
    // first on the `xargs` first-token basename — the wrapper itself
    // is the escape hatch regardless of what is being wrapped.
    let (_dir, root) = setup_repo_on_branch("main");
    let input = r#"{"tool_input": {"command": "xargs git commit"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&root));
    assert_eq!(code, 2, "xargs is blocked at Layer 7.5; stderr={stderr}");
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
    // perform a commit through Claude's Bash tool path. Layer 9
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
    // main and must not be blocked by Layer 9.
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
    // effective cwd onto the integration branch. Layer 9 must
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
    // of the inner content. Layer 7.5
    // (`.claude/rules/no-escape-hatches.md`) fires before Layer 9's
    // integration-branch matcher unwraps the `-c` argument, so the
    // block message is the escape-hatch citation rather than the
    // integration-branch citation. The intent of the original test
    // — `bash -c 'git commit ...'` is rejected on main — is preserved
    // by the earlier and stronger Layer 7.5 block.
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
    // returns Some(_) == "commit" → false. Layer 9 doesn't fire.
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
    // subcommand → Layer 9 does not fire → allow.
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
// Layer 9 also fires when the hook's effective cwd resolves to a
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

    if with_state_file {
        let states_dir = root.join(".flow-states").join(branch);
        std::fs::create_dir_all(&states_dir).unwrap();
        std::fs::write(states_dir.join("state.json"), "{}").unwrap();
    }

    let worktree = root.join(".worktrees").join(branch);
    std::fs::create_dir_all(&worktree).unwrap();
    // The .git pointer's target need not exist: detect_branch_from_path
    // recognizes the branch from the `.worktrees/<branch>/` path
    // segment alone, and current_branch_in's git subprocess fallback
    // failing here is the desired behavior — match_branch_at must
    // return None so the active-flow predicate is what fires.
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
    // Layer 7.5's structural escape-hatch block fires before Layer 10's
    // commit-during-flow gate because `bash -c` is itself a shell-eval
    // escape hatch regardless of what's wrapped inside it. The block
    // still fires; the message is the no-escape-hatches.md citation
    // rather than the active-flow citation. The test's intent —
    // `bash -c 'git commit ...'` is rejected during an active flow —
    // is preserved by the earlier and stronger Layer 7.5 block.
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
    // Read-only git is not a commit invocation → Layer 9 is silent.
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
    // → active-flow predicate returns None → Layer 9 silent.
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
/// unlinked. The hook must fall through Layer 9 cleanly (no panic,
/// no Layer 9 fire) and exit 0 on the allowed `git status` payload.
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
    // so match_branch_at returns None ("feat-x" != "main"). Layer 9
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
// the marker Layer 9 checks. When the carve-out fires, the hook
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
    // so Layer 9 passes through. CI runs inside finalize-commit and
    // the commit lands.
    let (_dir, root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let jsonl = assistant_skill_jsonl("flow:flow-commit");
    let transcript = crate::common::transcript_fixture(&root, "p", &jsonl);
    let input = format!(
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "/Users/me/code/flow/bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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
    // Layer 7.5 (`.claude/rules/no-escape-hatches.md`) blocks
    // `bash -c '...'` as a shell-eval escape hatch regardless of the
    // wrapped inner command. The active-flow carve-out at Layer 10
    // recognizes a `bash -c`-wrapped finalize-commit shape only for
    // callers that bypass Layer 7.5 — and during an active flow no
    // such caller exists, because /flow:flow-commit invokes
    // `bin/flow finalize-commit` directly via the Bash tool (no
    // bash -c wrapper). The skill-commit carve-out is therefore
    // unreachable from the active-flow path when the wrapper is
    // bash -c. The legitimate skill commit path bypasses Layer 7.5
    // because it does not pass through bash -c at all.
    let (_dir, _root, cwd) =
        setup_active_flow_worktree_with_state("feat", r#"{"_continue_pending": "commit"}"#);
    let input = r#"{"tool_input": {"command": "bash -c 'bin/flow finalize-commit msg.txt feat'"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input(input, Some(&cwd));
    assert_eq!(
        code, 2,
        "bash -c wrapper is itself a shell-eval escape hatch and must block at Layer 7.5; stderr={stderr}"
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

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt main"}}"#;
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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
    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt feat"}}"#;
    let (code, _stdout, stderr) = run_hook_with_input_and_home(input, Some(&cwd), Some(&root));
    assert_eq!(code, 2);
    assert!(stderr.contains("/flow:flow-commit"));
    assert!(stderr.contains("no-escape-hatches.md"));
}

// --- layer_10_bootstrap_carveout ---
//
// The bootstrap-skill carve-out on Layer 9's integration-branch
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
    // bootstrap-carveout conditions hold → Layer 9 passes through.
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&root), Some(&root));
    assert_eq!(
        code, 0,
        "bootstrap-skill carve-out must pass on main with flow-prime chain; stderr={stderr}"
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt staging"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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

    let input = r#"{"tool_input": {"command": "bin/flow finalize-commit msg.txt main"}}"#;
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt main"}}, "transcript_path": "{}"}}"#,
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
    // but the command carries a `-C <main_root>` token that shifts
    // git's effective cwd onto the integration branch. The matcher
    // checks BOTH candidate cwds: the hook's process cwd and any
    // `-C` target. The first match_branch_at(cwd) returns None for
    // the feature branch; the -C target's match_branch_at fires.
    // The bootstrap carve-out is intentionally NOT applied at the
    // -C target callsite (cwd-only design) because the transcript
    // walker is session-scoped: a bootstrap chain in session
    // activity for one repo could otherwise authorize a commit
    // redirected via -C to another repo's integration branch.
    // Legitimate bootstrap windows always run with cwd ON the
    // integration branch, so this tightening has no production
    // consumer cost.
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit -C {} msg.txt main"}}, "transcript_path": "{}"}}"#,
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
        r#"{{"tool_input": {{"command": "bin/flow finalize-commit msg.txt feat"}}, "transcript_path": "{}"}}"#,
        transcript.to_string_lossy()
    );
    let (code, _stdout, stderr) = run_hook_with_input_and_home(&input, Some(&cwd), Some(&root));
    assert_eq!(
        code, 0,
        "feature-branch active-flow carve-out path is independent of bootstrap chain; stderr={stderr}"
    );
}
