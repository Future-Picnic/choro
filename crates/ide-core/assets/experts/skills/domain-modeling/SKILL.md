---
name: domain-modeling
description: "Clarify entities, invariants, ownership, and API contracts for the backend change being implemented."
license: MIT
---

# Domain Modeling & Contracts

Use the project's existing domain vocabulary and actual code to distinguish concepts with different lifecycles or authority. Identify the invariants the assigned change must preserve, who owns each state transition, and which operations can be retried or run concurrently.

For an API contract, specify inputs, outputs, validation, authorization, error semantics, and compatibility with callers. Consider duplicate submissions, partial failure, transaction boundaries, and pagination when those affect the assignment. Adapt to the existing framework and database; no new platform is implied.

Check proposed terminology against existing types and documentation. Resolve consequential ambiguities through the lead in a delegated task. Record agreed contracts in the task report or existing project document. Create a glossary or architecture decision record only when requested or necessary to preserve an important decision; do not introduce a documentation workflow for every endpoint.
