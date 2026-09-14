---
name: ci-efficiency
description: "Improve the existing CI workflow without hiding required validation or changing account settings."
license: MIT
---

# CI Efficiency · Choro edition

Inspect the existing workflow, triggers, dependency cache keys, matrix, job dependencies, and concurrency. Use recent run logs when available; otherwise label conclusions as static inspection. Establish whether the problem is elapsed time, runner minutes, reliability, or repeated work.

Choose changes supported by evidence: correct lockfile-based caching, avoid duplicate runs, safely gate irrelevant paths, or parallelize independent jobs. Preserve required release, migration, platform, and shared-library checks. Never discard a supported platform solely to improve a metric. Distinguish runner minutes from end-to-end duration and estimates from measurements.

Validate locally with the project's tools. Remote pushes, workflow dispatch, billing changes, and deployments follow the user's existing authorization; permission to inspect or edit YAML alone does not authorize them. Return the focused changes, evidence, and remaining live checks.
