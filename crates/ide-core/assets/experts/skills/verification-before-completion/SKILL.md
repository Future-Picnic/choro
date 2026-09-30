---
name: verification-before-completion
description: Verify the requested change with current evidence before reporting it complete.
license: MIT
---

# Verification Before Completion · Choro edition

Choose the check that proves the requested behavior, then run it against the current implementation. Read the result and exit status. A model’s report, a successful edit, or an unrelated passing test is not evidence that the task is complete.

For a bug, reproduce the original failure when practical and verify the corrected path. For an implementation, check its acceptance criteria and affected integrations. For a build or test claim, identify the exact command and observed outcome. Preserve any project-required checks and explain tests that could not run.

Use evidence collected during this task if the relevant files and environment have not changed since the check. Repeat only when new changes, failures, or an unresolved concern invalidate it. Avoid rerunning a full suite for every status message, reverting working fixes for ceremony, or adding tests that only mirror the implementation.

Report what passed, what remains unverified, and any blockers. In a delegated task, provide that evidence in the structured completion report; the lead still verifies the combined result.
