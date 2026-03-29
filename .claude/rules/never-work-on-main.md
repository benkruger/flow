# Never Work on Main

Never edit code directly on the main branch. When a bug or feature
is identified, file an issue via `/flow:flow-create-issue` and then
use `/flow:flow-start` to begin a proper feature branch.

Working on main bypasses the entire FLOW lifecycle — no PR, no
review, no test gate, no learnings.
