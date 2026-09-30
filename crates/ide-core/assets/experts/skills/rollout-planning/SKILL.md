---
name: rollout-planning
description: "Prepare proportionate deployment, verification, and recovery steps for the existing environment."
license: MIT
---

# Rollout Planning · Choro edition

Identify the requested change, target environment, dependencies, state transitions, and failure impact. Use the repository's existing deployment mechanism and authentication. Do not introduce a new hosting service or credentials by default.

For a small change, provide prerequisites, deployment steps, health signals, and a realistic recovery path. Expand the plan only for concrete risks such as data migrations, incompatible versions, or multiple services. Distinguish reversible code rollout from irreversible data changes; do not promise rollback restores externally modified data.

Use the user's actual approval and maintenance constraints. Do not invent stakeholder gates, dates, cost estimates, monitoring windows, or on-call contacts. Plan-only work does not deploy. When execution is authorized, use the permitted environment, verify the specified result, and stop on unexplained health failures.
