#!/usr/bin/env bash
# FLOW-STUB-UNCONFIGURED (remove this line once you configure a real test runner)
# FLOW test runner — called by `bin/flow ci` (and `bin/flow ci --test`).
#
# Three invocation forms:
#   bin/test                                 — full suite
#   bin/test --file <path> [extra args...]   — single test file
#   bin/test --adversarial-path              — print canonical adversarial test path
#   bin/test [extra args...]                 — extra args forwarded as filters
#
# `--adversarial-path` returns the path the Code Review adversarial
# agent writes its probe test to. The path must live inside the
# project's test tree so the language test runner can discover and
# execute it; worktree removal at Phase 6 Complete disposes of the
# file as a side effect of removing the worktree directory. The
# unconfigured default exits 2 with a stderr message so Code Review
# halts cleanly until the project owner sets a real path.
#
# Recommended per-language values are listed alongside each example
# below — pick one set, uncomment the runner line, and edit the
# `--adversarial-path` echo line to match.
#
# Examples (uncomment one set):
#   exec cargo nextest run --status-level none --final-status-level fail "$@"
#       # --adversarial-path: tests/test_adversarial_flow.rs
#   exec python3 -m pytest "$@"
#       # --adversarial-path: tests/test_adversarial_flow.py
#   exec go test ./... "$@"
#       # --adversarial-path: adversarial_flow_test.go
#   exec bundle exec rails test "$@"
#       # --adversarial-path: test/adversarial_flow_test.rb
#   exec bundle exec rspec "$@"
#       # --adversarial-path: spec/adversarial_flow_spec.rb
#   exec npx jest "$@"
#       # --adversarial-path: tests/test_adversarial_flow.test.ts
#   exec swift test
#       # --adversarial-path: Tests/AdversarialFlowTests.swift

if [ "$1" = "--adversarial-path" ]; then
    echo "bin/test: --adversarial-path not configured (stub) — edit $0" >&2
    exit 2
fi

echo "bin/test: no test runner configured (stub) — edit $0" >&2
exit 0
