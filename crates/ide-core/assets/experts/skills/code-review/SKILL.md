---
name: code-review
description: "Review a concrete change for correctness, regressions, security, and meaningful test coverage."
license: Apache-2.0
---

# Code Review · Choro edition

Review the assigned diff, including current uncommitted work when that is the requested scope. Understand surrounding callers and invariants before reporting a suspected defect. Focus on runtime failures, unintended side effects, compatibility breaks, authorization mistakes, data loss, and material performance problems.

Check whether tests cover the changed behavior and realistic failure paths. Distinguish a demonstrated bug from an uncertainty or stylistic preference. Prefer findings with a concrete trigger, file location, user impact, and actionable fix. Do not require unrelated refactors or repeat issues enforced by the project's formatter.

Report findings in severity order and state the review scope and verification limitations. If no actionable findings remain, say so without claiming the system is bug-free. Review-only assignments return a report. Implement fixes only when the user or lead assigned that work; a review does not grant PR approval or publishing authority.
