# iOS Conventions

## Architecture Patterns

- **SwiftUI** — Read the full view hierarchy and data flow before modifying.
  Check for `@Observable`, `@State`, `@Binding`, and `@Environment` usage.
- **Dependency injection** — Use constructor injection with protocols. No DI
  frameworks. Check existing protocol definitions before creating new ones.
- **Concurrency** — Use structured concurrency (`async`/`await`, `TaskGroup`).
  Check for `@MainActor` annotations on UI-facing types. Never use
  `DispatchQueue` for new code.

## Test Conventions

- Use Swift Testing framework (`import Testing`) unless the project uses
  XCTest. Check existing test files for the convention.
- Never run `xcodebuild` directly — use `bin/flow ci` or `bin/flow test`.
- Targeted test command: `bin/flow test -- ClassName` or `bin/flow test -- ClassName/testName`

## CI Failure Fix Order

1. Build errors — fix compilation errors first
2. Test failures — understand the root cause, fix the code not the test
3. Coverage gaps — write the missing test

## Hard Rules

- Never run `xcodebuild`, `xcrun`, or `xcresulttool` directly — use
  `bin/flow ci`, `bin/flow test`, or `bin/flow build` subcommands.
- Never modify `.xcodeproj/project.pbxproj` by hand — use Xcode.
- Always read the full protocol and its conformances before modifying.

## Dependency Management

- Run `bin/dependencies` to resolve packages.
