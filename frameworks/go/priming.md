# Go Conventions

## Architecture Patterns

- **Package structure** — Read the full package and its imports before modifying.
  Check for circular import risks and package-level init functions.
- **Interfaces** — Define interfaces at the point of use, not at the point of
  implementation. Keep interfaces small (1-3 methods).
- **Error handling** — Always check returned errors. Never use `_` to discard
  errors. Wrap errors with `fmt.Errorf("context: %w", err)` for stack context.
- **Concurrency** — Use goroutines and channels for concurrent work. Check for
  race conditions with `go test -race`. Never share memory without synchronization.

## Test Conventions

- Use the standard `testing` package. Check existing `_test.go` files for
  patterns before adding new tests.
- Use table-driven tests for functions with multiple input/output cases.
- Follow existing test patterns in the project.
- Targeted test command: `bin/test <path/to/package>`

## CI Failure Fix Order

1. Build errors — fix compilation errors first (`go build ./...`)
2. Vet warnings — fix static analysis issues (`go vet ./...`)
3. Test failures — understand the root cause, fix the code not the test
4. Coverage gaps — write the missing test

## Hard Rules

- Always run `go vet ./...` before committing — it catches common mistakes.
- Never disable or skip vet checks.
- Always read the full interface and its implementations before modifying.

## Dependency Management

- Run `bin/dependencies` to tidy modules (`go mod tidy`).
