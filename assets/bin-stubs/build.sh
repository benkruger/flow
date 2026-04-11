#!/usr/bin/env bash
# FLOW build — called by `bin/flow build` and `bin/flow ci`.
#
# Compiles your project; nonzero exit blocks the rest of the CI
# sequence. This stub does nothing yet — uncomment one of the lines
# below for your toolchain and delete the rest. The exit-0 default
# keeps fresh primes from blocking CI; you will see the reminder on
# every CI run until you configure a real build step. Languages
# without a separate build step (Python, Ruby/Rails) can leave this
# as exit 0 indefinitely.
#
# Examples:
#   exec cargo build --quiet
#   exec go build ./...
#   exec npm run build
#   exec swift build
#   exec ./gradlew build

echo "bin/build: no build step configured (stub) — edit $0" >&2
exit 0
