# Rust Conventions

## Architecture Patterns

- **Module structure** — Read the full module and its imports before modifying.
  Check for circular dependency risks and module-level statics.
- **Ownership and borrowing** — Understand ownership, borrowing, and lifetimes
  before modifying function signatures. Prefer borrowing (`&T`, `&mut T`) over
  cloning unless ownership transfer is required.
- **Error handling** — Use `Result<T, E>` for recoverable errors. Never use
  `.unwrap()` in production code — use `?` operator or explicit `match`/`if let`.
  Define custom error types with `thiserror` or manual `impl` when the crate
  uses them.
- **Traits** — Check existing trait implementations before adding new ones.
  Prefer deriving standard traits (`Debug`, `Clone`, `PartialEq`) where possible.

## Test Conventions

- Use the built-in `#[cfg(test)]` module with `#[test]` attribute functions.
  Check existing test modules for patterns before adding new tests.
- Use `assert_eq!`, `assert_ne!`, and `assert!` macros for assertions.
- Follow existing test patterns in the project.
- Targeted test command: `bin/test <test_name>`

## CI Failure Fix Order

1. Compilation errors — fix build errors first (`cargo build`)
2. Clippy warnings — fix lint issues (`cargo clippy -- -D warnings`)
3. Format violations — fix formatting (`cargo fmt`)
4. Test failures — understand the root cause, fix the code not the test
5. Coverage gaps — write the missing test

## Hard Rules

- Always run `cargo clippy -- -D warnings` before committing — warnings fail the build.
- Never allow clippy warnings — fix them or explicitly document exceptions with `#[allow(...)]`.
- Always read the full trait and its implementations before modifying.

## Dependency Management

- Run `bin/dependencies` to update packages (`cargo update`).
