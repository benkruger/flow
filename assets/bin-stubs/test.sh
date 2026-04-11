#!/usr/bin/env bash
# FLOW-STUB-UNCONFIGURED (remove this line once you configure a real test runner)
# FLOW test runner — called by `bin/flow test` and `bin/flow ci`.
#
# Two invocation forms:
#   bin/test                                 — full suite
#   bin/test --file <path> [extra args...]   — single test file
#   bin/test [extra args...]                 — extra args forwarded as filters
#
# This stub does nothing yet — uncomment one of the lines below for
# your toolchain and delete the rest. The exit-0 default keeps fresh
# primes from blocking CI; you will see the reminder on every CI run
# until you configure a real test runner.
#
# Examples:
#   exec cargo nextest run --status-level none --final-status-level fail "$@"
#   exec python3 -m pytest "$@"
#   exec go test ./... "$@"
#   exec bundle exec rails test "$@"
#   exec bundle exec rspec "$@"
#   exec npx jest "$@"
#   exec swift test

echo "bin/test: no test runner configured (stub) — edit $0" >&2
exit 0
