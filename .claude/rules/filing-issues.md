# Filing Issues

## The Pattern

1. Write the issue body to `.flow-issue-body` in the project
   root using the Write tool
2. Call `bin/flow issue --title "..." --body-file .flow-issue-body`
3. The script reads the file, deletes it, then creates the issue

## Rules

- Never pass body text as a command line argument — special
  characters trigger the Bash hook validator
- Never delete `.flow-issue-body` yourself — the script handles
  cleanup after reading
- Always use `bin/flow issue` — never call `gh issue create`
  directly
