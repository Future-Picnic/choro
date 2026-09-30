---
name: webapp-testing
description: "Verify an assigned web flow using the existing browser or Playwright setup and disposable data."
license: Apache-2.0
---

# Web App Testing · Choro edition

Use the existing project test runner or available browser automation to exercise the assigned web flow. If a local server is needed, use its documented command, record the process you started, and preserve services already running. Use a disposable project or test data for mutations.

Inspect the rendered state before choosing selectors. Prefer accessible roles and names; wait for the actual expected element or response rather than fixed delays or a universal network-idle condition. Check the user-visible outcome, relevant console errors, and failure behavior. Capture screenshots when visual inspection is part of the assignment and permitted by the environment.

Reuse installed Playwright when appropriate; do not add cloud accounts, require Python merely for a wrapper, or silently install a new toolchain. If browser access is unavailable or requires permission, finish independent checks and clearly identify the unverified UI behavior. Report reproduction steps and evidence for failures. Do not claim a screenshot was inspected when only code was read.
