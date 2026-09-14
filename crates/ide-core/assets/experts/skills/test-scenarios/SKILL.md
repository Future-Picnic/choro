---
name: test-scenarios
description: "Design compact acceptance checks for complete user journeys and independently failing behavior."
license: MIT
---

# Test Scenarios

Map the assigned requirements to complete user journeys with explicit setup, action, and observable outcome. Include distinct failure paths, invalid input, boundary conditions, and permission differences where they can independently fail.

Prioritize the highest-value flows. Merge adjacent steps that can be verified in one pass; avoid a case for every click. Use disposable fixtures and existing test infrastructure. Record which checks were executed and their outcomes, which remain manual, and blockers. A planned scenario is not a passed test. Scale the checklist to the change and avoid arbitrary counts or invented performance thresholds.
