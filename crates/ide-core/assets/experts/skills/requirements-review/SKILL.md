---
name: requirements-review
description: "Compare implemented behavior with the originating request and documented project conventions."
license: MIT
---

# Requirements Review

Read the actual user assignment and applicable specification, then trace each material requirement to its implementation and verification. Check missing behavior, edge cases, unexpected scope expansion, and regressions in adjacent flows. An implementation's own comments are not proof that a requirement is satisfied.

Separately check relevant documented project conventions. Treat architecture smells as hypotheses and suppress preferences contradicted by an intentional project design. Consider untracked and unstaged files when reviewing work in progress.

Report requirement gaps and code defects with evidence, location, and consequence. When no spec exists, use the user's request and state what remains ambiguous. Perform the review within this assigned agent; do not start native reviewers, set up an issue tracker, publish findings, or apply changes unless that action is part of the assignment.
