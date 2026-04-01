# Agents Quick Reference

## Bead Commands

```bash
bd ready                              # See unblocked work
bd show <id>                          # Bead details
bd update <id> --status in_progress   # Claim a bead
bd update <id> --status ready_for_qa  # Submit for QA
bd comments add <id> "message"        # Add a comment
bd list                               # See all beads
```

## Landing the Plane (End of Session)

1. Commit all work in progress
2. Comment current state on any in-progress beads
3. Push all branches
4. Report status to super

## Communication

```bash
initech send <agent> "message"    # Send message to an agent
initech peek <agent>              # Read agent terminal output
```
