# Rails Conventions

## Architecture Patterns

- **Model hierarchy** — Models use a Base/Create split. Always read the full
  class hierarchy (model, parent class, ApplicationRecord) before modifying.
- **Callbacks** — Check for `before_save`, `after_create`, `before_destroy`
  and all other callbacks. If `update!` or `save` will be called, check if
  callbacks will overwrite your values.
- **Soft deletes** — Check for `default_scope` and use `.unscoped` where needed.
- **Workers** — Structure: `pre_perform!` (load/validate, call `halt!` to stop),
  `perform!` (main work), `post_perform!` (cleanup/notifications). Queue names
  come from `config/sidekiq.yml`.
- **Controllers** — Params via `options` (OpenStruct). Responses:
  `render_ok`, `render_error`, `render_unauthorized`, `render_not_found`.
  Check which subdomain's BaseController to inherit from.
- **Routes** — Always use `scope` with `module:`, `as:`, `controller:`,
  `action:` explicitly. Never raw paths — always named route helpers. Check
  `config/routes/` for the correct file.

## Test Conventions

- Search `test/support/` for existing `create_*!` helpers before creating
  new ones.
- Never use `Model::Create.create!` directly — always use `create_*!` helpers.
- Never use `update_column` — always use `update!`.
- Read the mailer template if testing a mailer — all fields it references
  must be populated in fixtures.
- Targeted test command: `bin/rails test <test/path/to/file_test.rb>`

## CI Failure Fix Order

1. RuboCop violations — run `rubocop -A` first, then manual fixes
2. Test failures — understand the root cause, fix the code not the test
3. Coverage gaps — write the missing test

## Hard Rules

- Never disable a RuboCop cop (`# rubocop:disable`) without direct user
  approval.
- Never modify `.rubocop.yml` — fix the code, not the configuration.
- Always read the full class hierarchy before touching any model.

## Dependency Management

- Run `bin/dependencies` to update gems (`bundle update --all`).
