# GitHub Dependencies

## Finding What Blocks or Is Blocked By an Issue

Resolve dependencies in this order. Stop at the first source
that returns results.

1. **GraphQL API** — `blocking` and `blockedBy` fields on the
   Issue type. These return `IssueConnection` with actual
   dependency edges.

   ```graphql
   issue(number: N) {
     blocking(first: 10) { nodes { number title state } }
     blockedBy(first: 10) { nodes { number title state } }
   }
   ```

2. **Issue body** — if GraphQL returns empty, check the issue
   body for explicit mentions of blocking relationships.

3. **"Blocked" label** — if neither above returns results, the
   label indicates the issue is blocked but the specific
   blocker is unknown.

## Never

- Never skip straight to the issue body or label when the
  GraphQL API is available
- Never guess dependencies from phase labels or titles
